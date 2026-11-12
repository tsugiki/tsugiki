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
    /// The Origin PRs that merged with none of their Backports open, and the merged Backports listed on
    /// their own, which the view folds into one line (#588).
    pub merged: Vec<TreeEntry>,
    /// How many PRs `merged` holds, Backports included.
    pub merged_count: u32,
    /// Nothing, for an Issue whose author's account has gone.
    pub author: Option<Login>,
    /// Whom it is assigned to, as the sync last read them (#595).
    pub assignees: Vec<Login>,
    /// Nothing, until Tsugiki has read this Issue from GitHub once.
    pub content: Option<IssueContent>,
    /// The PRs and Backports in its tree that Need attention, each by itself (#597).
    pub needs_attention: BTreeSet<u32>,
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
        let assignees = held
            .map(|issue| issue.assignees.clone())
            .unwrap_or_default();
        let matched = FilterFacts::new(repository, std::mem::take(&mut facts.filters)).matched();
        let causes = Causes::of(&facts, repository.conventions);
        let found = candidates(facts, repository, &BTreeSet::new(), &causes)
            .into_iter()
            .find_map(|item| match item {
                WorkbenchItem::Issue(issue) if issue.number == number => Some(issue),
                WorkbenchItem::Issue(_)
                | WorkbenchItem::StandalonePr(_)
                | WorkbenchItem::StandaloneBackport(_) => None,
            });
        Ok(found.map(|mut issue| {
            let needs_attention = issue
                .rows()
                .into_iter()
                .map(|row| row.number)
                .filter(|&pr| causes.needs_attention(pr))
                .collect();
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
                assignees,
                content,
                needs_attention,
            }
        }))
    }
}

/// Splits an Issue's tree into the entries the view lists and the ones it folds. An Origin PR folds once it
/// has merged and none of its Backports is open: a Backport closed unmerged is done too, as when a manual one
/// replaced a bot's that conflicted. A Backport listed on its own folds once it has merged. Either stays
/// listed when one of its PRs Needs attention or a Filter matched it, which `stays` says of each number.
fn fold(entries: Vec<TreeEntry>, stays: impl Fn(u32) -> bool) -> (Vec<TreeEntry>, Vec<TreeEntry>) {
    entries.into_iter().partition(|entry| {
        let done = match entry {
            TreeEntry::Pr(tree) => {
                let beneath = tree.backports.iter().map(|backport| &backport.row);
                tree.row.state == PrState::Merged
                    && beneath.into_iter().all(|row| row.state != PrState::Open)
            }
            TreeEntry::Backport(backport) => backport.row.state == PrState::Merged,
        };
        !done || entry.rows().iter().any(|row| stays(row.number))
    })
}

#[cfg(test)]
mod tests;
