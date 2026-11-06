//! A Repository: its name, its Conventions, and what adding it would take (ADR 0011, ADR 0015).

use std::fmt;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::conventions::Conventions;
use crate::error::{Error, ErrorKind};
use crate::filters::Filter;

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

/// A Repository the user added, as `settings.toml` holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(from = "RepositoryInFile")]
pub struct RepositorySettings {
    pub name: RepositoryName,
    pub conventions: Conventions,
    pub filters: Vec<Filter>,
}

/// The user's standing in a Repository, as far as adding it needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryAccess {
    /// The user administers it on GitHub.
    Maintained,
    Other,
}

impl RepositorySettings {
    /// The settings adding `name` gives it, for a user with this `access`.
    pub fn added(name: RepositoryName, access: RepositoryAccess) -> Self {
        let conventions = Conventions::for_repository(&name);
        Self {
            name,
            conventions,
            filters: Filter::starters(access),
        }
    }

    /// A Repository Tsugiki knows nothing else about.
    pub fn new(name: RepositoryName) -> Self {
        Self::added(name, RepositoryAccess::Other)
    }
}

/// A Repository added by hand may leave its Conventions and its Filters out. Then it gets what adding it
/// would give it, as far as the file can say: only GitHub knows if the user administers it. `filters = []`
/// means no Filters.
#[derive(Deserialize, Type)]
struct RepositoryInFile {
    name: RepositoryName,
    conventions: Option<Conventions>,
    filters: Option<Vec<Filter>>,
}

impl From<RepositoryInFile> for RepositorySettings {
    fn from(file: RepositoryInFile) -> Self {
        let conventions = file
            .conventions
            .unwrap_or_else(|| Conventions::for_repository(&file.name));
        let filters = file
            .filters
            .unwrap_or_else(|| Filter::starters(RepositoryAccess::Other));
        Self {
            name: file.name,
            conventions,
            filters,
        }
    }
}

/// Where the token comes from. A personal access token joins this with the Account page (ADR 0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TokenSource {
    Gh,
}

/// What the window opens on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "screen", rename_all = "snake_case")]
pub enum Home {
    /// No Repositories yet.
    Onboarding { token: TokenSource },
    /// The last Repository the user had open.
    Repository { repository: RepositorySettings },
}

impl Default for Home {
    fn default() -> Self {
        Self::Onboarding {
            token: TokenSource::Gh,
        }
    }
}

/// Up to this many open Issues and PRs loads everything open, and more loads a window (ADR 0015).
pub(crate) const SMALL_UP_TO: u32 = 500;
/// How far back the first sync looks, in days (ADR 0015).
pub const WINDOW_DAYS: u8 = 30;

/// Measured on CPython's last 30 days (#132): 4,760 items took about 485 points and 20.5 minutes, one request
/// at a time, with whole trees for the user's own Issues. About 10 of those points are the user's own work
/// and don't grow with the window.
const FIXED_POINTS: u32 = 10;
const POINTS_PER_THOUSAND_ITEMS: u32 = 100;
const MILLISECONDS_PER_ITEM: u32 = 260;

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
}

/// What the add dialog shows before the user decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RepositoryCheck {
    pub repository: RepositorySettings,
    pub access: RepositoryAccess,
    pub open_issues: u32,
    pub open_prs: u32,
    pub size: RepositorySize,
    pub window_days: u8,
    /// How many Issues and PRs the first sync loads.
    pub first_sync_items: u32,
    pub first_sync_seconds: u32,
    pub first_sync_points: u32,
}

impl RepositoryCheck {
    pub fn new(name: RepositoryName, counts: RepositoryCounts, access: RepositoryAccess) -> Self {
        let open = counts.open_issues.saturating_add(counts.open_prs);
        let (size, items) = if open <= SMALL_UP_TO {
            (
                RepositorySize::Small,
                open.saturating_add(counts.closed_in_window),
            )
        } else {
            (RepositorySize::Large, counts.updated_in_window)
        };
        Self {
            repository: RepositorySettings::added(name, access),
            access,
            open_issues: counts.open_issues,
            open_prs: counts.open_prs,
            size,
            window_days: WINDOW_DAYS,
            first_sync_items: items,
            first_sync_seconds: items.saturating_mul(MILLISECONDS_PER_ITEM) / 1000,
            first_sync_points: FIXED_POINTS
                .saturating_add(items.saturating_mul(POINTS_PER_THOUSAND_ITEMS) / 1000),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> RepositoryName {
        RepositoryName::parse(text).unwrap()
    }

    const FEW: RepositoryCounts = RepositoryCounts {
        open_issues: 10,
        open_prs: 3,
        updated_in_window: 43,
        closed_in_window: 35,
    };

    #[test]
    fn a_name_is_read_from_what_a_user_types_or_pastes() {
        for text in [
            "python/cpython",
            "  python/cpython\n",
            "python/cpython/",
            "python/cpython.git",
            "github.com/python/cpython",
            "https://github.com/python/cpython",
            "https://github.com/python/cpython/",
            "https://github.com/python/cpython.git",
        ] {
            let parsed = name(text);
            assert_eq!(parsed.owner(), "python", "{text}");
            assert_eq!(parsed.name(), "cpython", "{text}");
        }
    }

    #[test]
    fn a_name_can_hold_what_github_allows() {
        assert_eq!(
            name("some-maintainer/pep848-sim").to_string(),
            "some-maintainer/pep848-sim"
        );
        assert_eq!(name("a/b_c.d-e").to_string(), "a/b_c.d-e");
    }

    #[test]
    fn anything_else_is_refused_before_it_reaches_github() {
        let long_owner = format!("{}/name", "o".repeat(40));
        let long_name = format!("owner/{}", "n".repeat(101));
        for text in [
            "",
            "python",
            "/cpython",
            "python/",
            "python/cpython/issues",
            "python/..",
            "python/.",
            "py thon/cpython",
            "python/cpy thon",
            "py_thon/cpython",
            "python/\"cpython\"",
            "example.com/python/cpython",
            "http://github.com/python/cpython",
            &long_owner,
            &long_name,
        ] {
            let problem = RepositoryName::parse(text).unwrap_err();
            assert_eq!(problem.kind, ErrorKind::NotFound, "{text}");
            assert_eq!(problem.message, BAD_NAME, "{text}");
        }
    }

    #[test]
    fn the_longest_names_github_allows_are_kept() {
        let longest = format!("{}/{}", "o".repeat(39), "n".repeat(100));
        assert_eq!(name(&longest).to_string(), longest);
    }

    #[test]
    fn case_doesnt_make_another_repository() {
        assert!(name("Python/CPython").is_same(&name("python/cpython")));
        assert!(!name("python/cpython").is_same(&name("python/peps")));
    }

    #[test]
    fn a_name_travels_as_a_plain_string_and_is_checked_on_the_way_in() {
        assert_eq!(
            serde_json::to_value(name("python/cpython")).unwrap(),
            "python/cpython"
        );
        assert_eq!(
            serde_json::from_str::<RepositoryName>(r#""python/cpython""#).unwrap(),
            name("python/cpython")
        );
        let problem = serde_json::from_str::<RepositoryName>(r#""nonsense""#).unwrap_err();
        assert!(problem.to_string().contains(BAD_NAME), "{problem}");
    }

    #[test]
    fn conventions_a_file_leaves_out_come_from_the_name() {
        let read = |text: &str| toml_edit::de::from_str::<RepositorySettings>(text).unwrap();
        assert_eq!(
            read(r#"name = "python/cpython""#).conventions,
            Conventions::Cpython
        );
        assert_eq!(
            read(r#"name = "python/peps""#).conventions,
            Conventions::Github
        );
        assert_eq!(
            read("name = \"python/cpython\"\nconventions = \"github\"").conventions,
            Conventions::Github
        );
    }

    #[test]
    fn up_to_500_open_is_small_and_loads_those_and_what_closed_lately() {
        let check =
            RepositoryCheck::new(name("some-maintainer/gcmon"), FEW, RepositoryAccess::Other);
        assert_eq!(check.size, RepositorySize::Small);
        assert_eq!(check.open_issues, 10);
        assert_eq!(check.open_prs, 3);
        assert_eq!(check.first_sync_items, 48);
        assert_eq!(check.window_days, 30);

        let at_the_limit = RepositoryCounts {
            open_issues: 400,
            open_prs: 100,
            ..FEW
        };
        let check = RepositoryCheck::new(name("a/b"), at_the_limit, RepositoryAccess::Other);
        assert_eq!(check.size, RepositorySize::Small);
        assert_eq!(check.first_sync_items, 535);
    }

    #[test]
    fn above_500_open_is_large_and_loads_the_window() {
        let just_over = RepositoryCounts {
            open_issues: 401,
            open_prs: 100,
            ..FEW
        };
        let check = RepositoryCheck::new(name("a/b"), just_over, RepositoryAccess::Other);
        assert_eq!(check.size, RepositorySize::Large);
        assert_eq!(check.first_sync_items, 43);
    }

    #[test]
    fn the_estimate_for_cpython_agrees_with_what_was_measured() {
        // A first sync measured about 485 points and 20.5 minutes for 4,760 items.
        let cpython = RepositoryCounts {
            open_issues: 6935,
            open_prs: 2308,
            updated_in_window: 4715,
            closed_in_window: 2506,
        };
        let check = RepositoryCheck::new(name("python/cpython"), cpython, RepositoryAccess::Other);
        assert_eq!(check.repository.conventions, Conventions::Cpython);
        assert!((460..=510).contains(&check.first_sync_points), "{check:?}");
        assert!(
            (1150..=1300).contains(&check.first_sync_seconds),
            "{check:?}"
        );
    }
}
