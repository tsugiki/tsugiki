//! An Issue on the Workbench, with its tree and its people.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use super::{TreeEntry, WorkbenchItem};
use crate::conventions::Conventions;
use crate::github::issues::{IssueFacts, IssueState};
use crate::login::Login;

/// An Issue on the Workbench, with its tree: each PR Tsugiki knows that fixes it, whatever its state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IssueItem {
    pub number: u32,
    pub title: String,
    /// Nothing for an Issue stored before Tsugiki kept states, until GitHub sends it again.
    pub state: Option<IssueState>,
    /// The Issue's author, then the PRs' authors, with each person once and no backport bot.
    pub people: Vec<Login>,
    /// The latest update of the Issue and its PRs.
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub updated_at: OffsetDateTime,
    pub pull_requests: Vec<TreeEntry>,
}

pub(super) fn issue_item(
    issue: IssueFacts,
    pull_requests: Vec<TreeEntry>,
    conventions: Conventions,
) -> WorkbenchItem {
    let people = people(&issue, &pull_requests, conventions);
    let updated_at = pull_requests
        .iter()
        .flat_map(TreeEntry::rows)
        .map(|row| row.updated_at)
        .fold(issue.updated_at, Ord::max);
    WorkbenchItem::Issue(IssueItem {
        number: issue.number,
        title: issue.title,
        state: issue.state,
        people,
        updated_at,
        pull_requests,
    })
}

fn people(issue: &IssueFacts, pull_requests: &[TreeEntry], conventions: Conventions) -> Vec<Login> {
    let authors = pull_requests
        .iter()
        .flat_map(TreeEntry::rows)
        .filter_map(|row| row.author.as_ref());
    let mut people: Vec<Login> = Vec::new();
    for author in issue.author.iter().chain(authors) {
        if !conventions.is_backport_bot(author) && !people.contains(author) {
            people.push(author.clone());
        }
    }
    people
}

#[cfg(test)]
mod tests;
