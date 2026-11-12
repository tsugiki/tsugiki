//! The Issue view: an Issue with its tree, and what it says (ADR 0008).

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

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
    /// Whom it is assigned to, as the sync last read them (#595).
    pub assignees: Vec<Login>,
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
        let (facts, content) = reader.issue_facts(&repository.name, number)?;
        let held = facts.issues.iter().find(|issue| issue.number == number);
        let author = held.and_then(|issue| issue.author.clone());
        let assignees = held
            .map(|issue| issue.assignees.clone())
            .unwrap_or_default();
        let found = candidates(facts, repository, &BTreeSet::new())
            .into_iter()
            .find_map(|item| match item {
                WorkbenchItem::Issue(issue) if issue.number == number => Some(issue),
                WorkbenchItem::Issue(_)
                | WorkbenchItem::StandalonePr(_)
                | WorkbenchItem::StandaloneBackport(_) => None,
            });
        Ok(found.map(|issue| Self {
            issue,
            author,
            assignees,
            content,
        }))
    }
}

#[cfg(test)]
mod tests;
