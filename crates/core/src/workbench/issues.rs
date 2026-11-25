//! An Issue on the Workbench, with its tree and its people.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use super::attention::seen_of;
use super::{MuteAuthor, Seen, TreeEntry, WorkbenchItem};
use crate::conventions::Conventions;
use crate::facts::issues::{IssueFacts, IssueState};
use crate::login::Login;
use crate::work::Causes;

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
    /// The Issue's own last update, apart from its PRs', which the row's tooltip names (#1066).
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub issue_updated_at: OffsetDateTime,
    pub pull_requests: Vec<TreeEntry>,
    /// Its part in Mark Seen, which covers the Issue and every PR in its tree.
    pub seen: Seen,
    /// What Mute author offers for the Issue's author (#494).
    pub mute_author: MuteAuthor,
    /// Whether the user pinned the Issue (#488), which its ★ shows.
    pub pinned: bool,
}

/// The Issue `issue` with its tree `pull_requests`, its part in Mark Seen and Mute author as `causes` judge
/// them.
pub(super) fn issue_item(
    issue: IssueFacts,
    pull_requests: Vec<TreeEntry>,
    conventions: Conventions,
    causes: &Causes,
) -> WorkbenchItem {
    let tree = (pull_requests.iter())
        .flat_map(TreeEntry::rows)
        .map(|row| row.number);
    let seen = seen_of(causes, std::iter::once(issue.number).chain(tree).collect());
    let mute_author = MuteAuthor::of(causes, issue.author.as_ref());
    let pinned = causes.is_pinned(issue.number);
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
        issue_updated_at: issue.updated_at,
        pull_requests,
        seen,
        mute_author,
        pinned,
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
