//! What Tsugiki knows about how particular Repositories work (ADR 0011).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::github::{Login, gh_number};
use crate::repository::RepositoryName;

const CPYTHON: &str = "python/cpython";
/// `pub` because the recorder keeps this login, which the rules have to recognise in a recording.
pub const CPYTHON_BACKPORT_BOT: &str = "miss-islington";

/// How a Repository's PRs name their Issues and Backports (ADR 0011). `links.rs` reads this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Conventions {
    Cpython,
    Github,
}

impl Conventions {
    const ALL: [Self; 2] = [Self::Cpython, Self::Github];

    /// `python/cpython` starts with CPython's, and every other Repository with GitHub's.
    pub fn for_repository(name: &RepositoryName) -> Self {
        if name.to_string().eq_ignore_ascii_case(CPYTHON) {
            Self::Cpython
        } else {
            Self::Github
        }
    }

    /// The Issues `title` names under these Conventions. GitHub's have no title prefix.
    pub fn issues_in_title(self, title: &str) -> Vec<u32> {
        match self {
            Self::Cpython => cpython_title_issues(title),
            Self::Github => Vec::new(),
        }
    }

    /// The Issues `title` could name under any Conventions.
    pub fn issues_in_title_under_any(title: &str) -> Vec<u32> {
        Self::ALL
            .iter()
            .flat_map(|conventions| conventions.issues_in_title(title))
            .collect()
    }

    /// True when this Repository's PRs can be Backports.
    pub fn has_backports(self) -> bool {
        match self {
            Self::Cpython => true,
            Self::Github => false,
        }
    }

    /// The Origin PR that `title` names with these Conventions' Backport marker.
    pub fn origin_in_title(self, title: &str) -> Option<u32> {
        match self {
            Self::Cpython => cpython_title_origin(title),
            Self::Github => None,
        }
    }

    /// The Origin PR that `title` could name under any Conventions.
    pub fn origin_in_title_under_any(title: &str) -> Option<u32> {
        Self::ALL
            .iter()
            .find_map(|conventions| conventions.origin_in_title(title))
    }

    /// True for a bot that opens Backports. A Backport it wrote is a Bot backport, and the Workbench leaves
    /// its name out of an Issue's people.
    pub fn is_backport_bot(self, login: &Login) -> bool {
        match self {
            Self::Cpython => login == &Login::new(CPYTHON_BACKPORT_BOT),
            Self::Github => false,
        }
    }
}

/// The Issues in a title such as `gh-1: Fix it`, `gh-1, gh-2: Fix both` or `[3.14] gh-1: Fix it (GH-9)`.
/// Anything else before the colon means the title has no prefix.
fn cpython_title_issues(title: &str) -> Vec<u32> {
    let Some((head, _)) = title.split_once(':') else {
        return Vec::new();
    };
    let head = head.trim_start();
    let names = head
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map_or(head, |(_, after)| after);
    let numbers: Option<Vec<u32>> = names
        .split([',', ' '])
        .filter(|name| !name.is_empty())
        .map(gh_number)
        .collect();
    numbers.unwrap_or_default()
}

/// The last `(GH-N)` in a title such as `[3.14] gh-1: Fix it (GH-9)`.
fn cpython_title_origin(title: &str) -> Option<u32> {
    title
        .rsplit('(')
        .find_map(|piece| gh_number(piece.split_once(')')?.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn for_name(text: &str) -> Conventions {
        Conventions::for_repository(&RepositoryName::parse(text).unwrap())
    }

    #[test]
    fn cpython_gets_its_conventions_and_others_get_githubs() {
        for cpython in ["python/cpython", "Python/CPython"] {
            assert_eq!(for_name(cpython), Conventions::Cpython);
        }
        for other in ["sergey-miryanov/gcmon", "python/peps", "someone/cpython"] {
            assert_eq!(for_name(other), Conventions::Github);
        }
    }

    #[test]
    fn a_cpython_title_names_its_issues_with_its_prefix() {
        for (title, expected) in [
            ("gh-123: Fix it", vec![123]),
            ("GH-123: Fix it", vec![123]),
            ("[3.14] gh-123: Fix it (GH-456)", vec![123]),
            ("gh-123, gh-456: Fix both", vec![123, 456]),
            ("gh-123 gh-456: Fix both", vec![123, 456]),
            ("[3.14] gh-123, GH-456: Fix both", vec![123, 456]),
            ("gh-123, see gh-456: Fix it", vec![]),
            (": Fix it", vec![]),
            ("Fix it", vec![]),
            ("Fix: gh-123 again", vec![]),
            ("see gh-123: it broke", vec![]),
            ("gh-: Fix it", vec![]),
            ("gh-12x: Fix it", vec![]),
            ("gé: Fix it", vec![]),
            ("[3.14 gh-123: Fix it", vec![]),
        ] {
            assert_eq!(
                Conventions::Cpython.issues_in_title(title),
                expected,
                "{title}"
            );
        }
    }

    #[test]
    fn a_title_names_nothing_under_githubs_conventions() {
        assert_eq!(
            Conventions::Github.issues_in_title("gh-123: Fix it"),
            [0_u32; 0]
        );
    }

    #[test]
    fn a_title_could_name_what_any_conventions_read_in_it() {
        assert_eq!(
            Conventions::issues_in_title_under_any("gh-123: Fix it"),
            [123]
        );
    }

    #[test]
    fn a_cpython_title_names_its_origin_pr_with_its_marker() {
        for (title, expected) in [
            ("[3.14] gh-1: Fix it (GH-9)", Some(9)),
            ("[3.14] Doc: Fix it (gh-9)", Some(9)),
            ("[3.14] gh-1: Fix it (GH-9) and more", Some(9)),
            ("[3.14] gh-1: Revert (GH-8) (GH-9)", Some(9)),
            ("[3.14] gh-1: Fix it (GH-9) (see notes)", Some(9)),
            ("[3.14] gh-1: Fix it", None),
            ("[3.14] gh-1: Fix it GH-9", None),
            ("[3.14] gh-1: Fix it (#9)", None),
            ("[3.14] gh-1: Fix it (GH-)", None),
            ("[3.14] gh-1: Fix it (GH-9", None),
            ("Fix it", None),
        ] {
            assert_eq!(
                Conventions::Cpython.origin_in_title(title),
                expected,
                "{title}"
            );
        }
    }

    #[test]
    fn githubs_conventions_have_no_backports() {
        let title = "[3.14] gh-1: Fix it (GH-9)";
        assert_eq!(Conventions::Github.origin_in_title(title), None);
        assert!(!Conventions::Github.has_backports());
        assert!(Conventions::Cpython.has_backports());
    }

    #[test]
    fn a_title_could_name_the_origin_any_conventions_read_in_it() {
        assert_eq!(
            Conventions::origin_in_title_under_any("[3.14] gh-1: Fix it (GH-9)"),
            Some(9)
        );
        assert_eq!(Conventions::origin_in_title_under_any("Fix it"), None);
    }

    #[test]
    fn cpython_has_a_backport_bot_and_github_has_none() {
        let bot = Login::new("Miss-Islington");
        assert!(Conventions::Cpython.is_backport_bot(&bot));
        assert!(!Conventions::Cpython.is_backport_bot(&Login::new("some-author")));
        assert!(!Conventions::Github.is_backport_bot(&bot));
    }
}
