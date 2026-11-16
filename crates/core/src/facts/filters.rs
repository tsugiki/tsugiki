//! Each Filter's last results (ADR 0015), as the store keeps them: which numbers belong to which search.

use std::collections::BTreeSet;

const CUT_OFF: &str = "GitHub lists at most 1,000 results for one search, and over 1,000 items here were \
                       created on one day, so this Filter may be missing some.";

/// What stopped a Filter's last search from being whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterProblem {
    /// More results than GitHub lists fell on one day, so the search read as far as GitHub goes.
    CutOff,
    /// GitHub refused the search, in these words.
    Refused(String),
}

impl FilterProblem {
    /// The Sidebar's words for this.
    pub fn words(&self) -> &str {
        match self {
            Self::CutOff => CUT_OFF,
            Self::Refused(words) => words,
        }
    }
}

/// A Filter's results as the last sync left them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterResults {
    /// The Issues and PRs it found, which share GitHub's one list of numbers.
    pub numbers: BTreeSet<u32>,
    pub problem: Option<FilterProblem>,
}
