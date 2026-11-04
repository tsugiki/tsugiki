//! What Tsugiki knows about how particular Repositories work (ADR 0011).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::github::Login;
use crate::repository::RepositoryName;

const CPYTHON: &str = "python/cpython";
const CPYTHON_BACKPORT_BOT: &str = "miss-islington";

/// How a Repository's PRs name their Issues and Backports (ADR 0011). `links.rs` reads this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Conventions {
    Cpython,
    Github,
}

impl Conventions {
    /// `python/cpython` starts with CPython's, and every other Repository with GitHub's.
    pub fn for_repository(name: &RepositoryName) -> Self {
        if name.to_string().eq_ignore_ascii_case(CPYTHON) {
            Self::Cpython
        } else {
            Self::Github
        }
    }

    /// CPython's PRs name their Issue with a `gh-NNNNN:` prefix, and GitHub's convention has no prefix.
    pub const fn names_issues_in_titles(self) -> bool {
        match self {
            Self::Cpython => true,
            Self::Github => false,
        }
    }

    /// True for a bot that opens PRs on someone's behalf. The Workbench leaves its name out of an Issue's
    /// people.
    pub fn is_backport_bot(self, login: &Login) -> bool {
        match self {
            Self::Cpython => login == &Login::new(CPYTHON_BACKPORT_BOT),
            Self::Github => false,
        }
    }
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
    fn cpython_has_a_backport_bot_and_github_has_none() {
        let bot = Login::new("Miss-Islington");
        assert!(Conventions::Cpython.is_backport_bot(&bot));
        assert!(!Conventions::Cpython.is_backport_bot(&Login::new("vstinner")));
        assert!(!Conventions::Github.is_backport_bot(&bot));
    }
}
