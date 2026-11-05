//! What Tsugiki knows about how particular Repositories work (ADR 0011).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::github::{Login, gh_number};
use crate::repository::RepositoryName;

const CPYTHON: &str = "python/cpython";
/// `pub` because the recorder keeps this login, which the rules have to recognise in a recording.
pub const CPYTHON_BACKPORT_BOT: &str = "miss-islington";
/// The GitHub App that speaks for the backport bot in comments.
pub const CPYTHON_BACKPORT_APP: &str = "miss-islington-app";
/// Accounts that look like people to GitHub and are bots.
pub const CPYTHON_BOT_ACCOUNTS: [&str; 2] = [CPYTHON_BACKPORT_BOT, "bedevere-bot"];
/// The words around the branch in the backport bot's two comments about a failure. If the bot's wording
/// changes, a failed Bot backport reads as a bot's comment and stops counting, until these change too.
const CPYTHON_BACKPORT_FAILURES: [(&str, &str); 2] = [
    (
        "I could not cleanly backport this to ",
        " due to a conflict",
    ),
    ("I had trouble checking out the ", " backport branch"),
];

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

    /// True for an account that GitHub takes for a person but these Conventions know is a bot.
    pub fn is_bot_account(self, login: &Login) -> bool {
        match self {
            Self::Cpython => CPYTHON_BOT_ACCOUNTS
                .iter()
                .any(|bot| login == &Login::new(bot)),
            Self::Github => false,
        }
    }

    /// The branch a Bot backport failed to reach, if `comment` from `author` says one did.
    pub fn failed_backport_target(self, author: &Login, comment: &str) -> Option<String> {
        match self {
            Self::Cpython if author == &Login::new(CPYTHON_BACKPORT_APP) => {
                CPYTHON_BACKPORT_FAILURES
                    .iter()
                    .find_map(|(before, after)| {
                        let (_, rest) = comment.split_once(before)?;
                        let (target, _) = rest.split_once(after)?;
                        Some(target.trim_matches('`').to_owned())
                    })
            }
            Self::Cpython | Self::Github => None,
        }
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
    fn cpython_knows_its_bot_accounts_and_github_knows_none() {
        for bot in ["miss-islington", "bedevere-bot"] {
            assert!(
                Conventions::Cpython.is_bot_account(&Login::new(bot)),
                "{bot}"
            );
            assert!(
                !Conventions::Github.is_bot_account(&Login::new(bot)),
                "{bot}"
            );
        }
        assert!(!Conventions::Cpython.is_bot_account(&Login::new("some-author")));
    }

    #[test]
    fn the_backport_bots_words_name_the_branch_it_failed_to_reach() {
        let app = Login::new("miss-islington-app");
        let target = |comment: &str| Conventions::Cpython.failed_backport_target(&app, comment);
        assert_eq!(
            target(
                "Sorry, @a and @b, I could not cleanly backport this to 3.14 due to a conflict."
            ),
            Some("3.14".to_owned())
        );
        assert_eq!(
            target("Sorry @a, I had trouble checking out the `3.13` backport branch."),
            Some("3.13".to_owned())
        );
        assert_eq!(
            target("Thanks @a for the PR, I'm working now to backport this."),
            None
        );
        assert_eq!(target("I could not cleanly backport this to 3.14"), None);
    }

    #[test]
    fn only_the_backport_bot_can_say_a_bot_backport_failed() {
        let words = "I could not cleanly backport this to 3.14 due to a conflict.";
        let person = Login::new("some-author");
        assert_eq!(
            Conventions::Cpython.failed_backport_target(&person, words),
            None
        );
        let app = Login::new("miss-islington-app");
        assert_eq!(
            Conventions::Github.failed_backport_target(&app, words),
            None
        );
    }

    #[test]
    fn cpython_has_a_backport_bot_and_github_has_none() {
        let bot = Login::new("Miss-Islington");
        assert!(Conventions::Cpython.is_backport_bot(&bot));
        assert!(!Conventions::Cpython.is_backport_bot(&Login::new("some-author")));
        assert!(!Conventions::Github.is_backport_bot(&bot));
    }
}
