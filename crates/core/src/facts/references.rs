//! The PRs that mention an Issue, which only the link rules can sort out (ADR 0011).

use crate::facts::issues::ListedPr;

/// The cross-references of one Issue, and the cursor of the older ones if a page didn't hold them all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueReferences {
    pub number: u32,
    /// Only PRs of this Repository, the newest first.
    pub listed: Vec<ListedPr>,
    pub next: Option<String>,
}
