//! The Issue view: an Issue with its tree, and what it says (ADR 0008).

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use super::attention::Causes;
use super::{IssueItem, WorkbenchItem, candidates};
use crate::error::Error;
use crate::github::issue_content::IssueContent;
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::store::StoreReader;

/// An Issue as the Detail pane shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IssueView {
    #[serde(flatten)]
    pub issue: IssueItem,
    /// Nothing, for an Issue whose author's account has gone.
    pub author: Option<Login>,
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
        let (facts, content) = reader.issue_facts(&repository.name, number)?;
        let held = facts.issues.iter().find(|issue| issue.number == number);
        let author = held.and_then(|issue| issue.author.clone());
        let causes = Causes::of(&facts, repository.conventions);
        let found = candidates(facts, repository, &BTreeSet::new())
            .into_iter()
            .find_map(|item| match item {
                WorkbenchItem::Issue(issue) if issue.number == number => Some(issue),
                WorkbenchItem::Issue(_)
                | WorkbenchItem::StandalonePr(_)
                | WorkbenchItem::StandaloneBackport(_) => None,
            });
        Ok(found.map(|issue| {
            let needs_attention = issue
                .rows()
                .into_iter()
                .map(|row| row.number)
                .filter(|&pr| causes.needs_attention(pr))
                .collect();
            Self {
                issue,
                author,
                content,
                needs_attention,
            }
        }))
    }
}

#[cfg(test)]
mod tests;
