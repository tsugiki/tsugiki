//! What Tsugiki knows about how particular Repositories work (ADR 0011).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::repository::RepositoryName;

const CPYTHON: &str = "python/cpython";

/// How a Repository's PRs name their Issues and Backports (ADR 0011). The rules that read this arrive with
/// the links.
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
}
