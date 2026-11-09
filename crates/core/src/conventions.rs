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
mod tests;
