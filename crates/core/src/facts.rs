//! The facts Tsugiki keeps about a Repository's Issues and PRs, which the rules judge. `github` parses
//! GitHub's answers into these types, the Mirror worker reads patch-ids into them, and the store fills its
//! bundles of them. They belong neither to GitHub's client nor to the store, so a module that judges facts
//! needs neither.

pub mod activity;
pub mod filters;
pub mod issue_content;
pub mod issues;
pub mod labels;
pub mod patch_ids;
pub mod pull_request_content;
pub mod pull_requests;
pub mod read_ahead;
pub mod references;

use std::collections::BTreeMap;

use serde::Serialize;

use crate::facts::activity::ActivityFacts;
use crate::facts::filters::FilterResults;
use crate::facts::issues::IssueFacts;
use crate::facts::patch_ids::ReviewFacts;
use crate::facts::pull_requests::PullRequestFacts;
use crate::filters::FilterQuery;
use crate::login::Login;

/// A commit's SHA in lower case, whole or cut short as a person wrote it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CommitSha(String);

/// The window reads a SHA as its text.
impl Serialize for CommitSha {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl CommitSha {
    /// Git's shortest default abbreviation, and the length of a whole SHA-1.
    const LENGTHS: std::ops::RangeInclusive<usize> = Self::SHORTEST..=40;

    /// Git's shortest default abbreviation.
    pub(crate) const SHORTEST: usize = 7;

    /// The SHA that `text` spells, if it spells one.
    pub fn parse(text: &str) -> Option<Self> {
        let is_sha =
            Self::LENGTHS.contains(&text.len()) && text.chars().all(|c| c.is_ascii_hexdigit());
        is_sha.then(|| Self(text.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True when `shorter` is this SHA or the start of it.
    pub fn starts_with(&self, shorter: &Self) -> bool {
        self.0.starts_with(&shorter.0)
    }
}

/// The facts a screen needs to work out the Workbench, read at one moment.
#[derive(Debug, PartialEq, Eq)]
pub struct WorkFacts {
    /// The user signed in when Tsugiki last reached GitHub, if it ever has.
    pub viewer: Option<Login>,
    pub pull_requests: Vec<PullRequestFacts>,
    pub issues: Vec<IssueFacts>,
    /// Each search of this Repository, with its last results.
    pub filters: BTreeMap<FilterQuery, FilterResults>,
    pub activity: ActivityFacts,
    pub reviews: ReviewFacts,
}
