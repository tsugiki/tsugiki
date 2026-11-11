//! The Issue view: an Issue with its tree, and what it says (ADR 0008).

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use super::attention::Causes;
use super::filters::FilterFacts;
use super::{IssueItem, TreeEntry, WorkbenchItem, candidates};
use crate::error::Error;
use crate::github::issue_content::IssueContent;
use crate::github::pull_requests::PrState;
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::store::StoreReader;

/// An Issue as the Detail pane shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IssueView {
    /// Its `pull_requests` leave out the ones in `merged`.
    #[serde(flatten)]
    pub issue: IssueItem,
    /// The Origin PRs that merged with each of their Backports, which the view folds into one line (#588).
    pub merged: Vec<TreeEntry>,
    /// How many PRs `merged` holds, Backports included.
    pub merged_count: u32,
    /// Nothing, for an Issue whose author's account has gone.
    pub author: Option<Login>,
    /// Nothing, until Tsugiki has read this Issue from GitHub once.
    pub content: Option<IssueContent>,
}

impl IssueView {
    /// Reads Issue `number`, or nothing if Tsugiki doesn't hold it. This waits on the database and never on
    /// GitHub, so the view opens on what was last read (ADR 0018).
    pub fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        number: u32,
    ) -> Result<Option<Self>, Error> {
        let (mut facts, content) = reader.issue_facts(&repository.name, number)?;
        let held = facts.issues.iter().find(|issue| issue.number == number);
        let author = held.and_then(|issue| issue.author.clone());
        let matched = FilterFacts::new(repository, std::mem::take(&mut facts.filters)).matched();
        let causes = Causes::of(&facts, repository.conventions);
        let found = candidates(facts, repository, &BTreeSet::new())
            .into_iter()
            .find_map(|item| match item {
                WorkbenchItem::Issue(issue) if issue.number == number => Some(issue),
                WorkbenchItem::Issue(_)
                | WorkbenchItem::StandalonePr(_)
                | WorkbenchItem::StandaloneBackport(_) => None,
            });
        Ok(found.map(|mut issue| {
            let stays = |pr: u32| matched.contains(&pr) || causes.needs_attention(pr);
            let (open, merged) = fold(std::mem::take(&mut issue.pull_requests), stays);
            let merged_count: usize = merged.iter().map(|entry| entry.rows().len()).sum();
            Self {
                issue: IssueItem {
                    pull_requests: open,
                    ..issue
                },
                merged,
                merged_count: u32::try_from(merged_count).unwrap_or(u32::MAX),
                author,
                content,
            }
        }))
    }
}

/// Splits an Issue's tree into the entries the view lists and the ones it folds: an Origin PR that merged
/// with each of its Backports, unless one of them Needs attention or a Filter matched it, which `stays`
/// says of each PR's number.
fn fold(entries: Vec<TreeEntry>, stays: impl Fn(u32) -> bool) -> (Vec<TreeEntry>, Vec<TreeEntry>) {
    entries.into_iter().partition(|entry| {
        let rows = entry.rows();
        !rows
            .iter()
            .all(|row| row.state == PrState::Merged && !stays(row.number))
    })
}

#[cfg(test)]
mod tests;
