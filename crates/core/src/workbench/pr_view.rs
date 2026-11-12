//! The PR view: a PR's row, and its Conversation (ADR 0008). The Files tab comes with the diff.

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use super::attention::Causes;
use super::{WorkbenchRow, candidates};
use crate::error::Error;
use crate::github::pull_request_content::PullRequestContent;
use crate::repository_settings::RepositorySettings;
use crate::store::StoreReader;

/// A PR as the Detail pane shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PrView {
    #[serde(flatten)]
    pub row: WorkbenchRow,
    /// Its description, comments and Reviews. Nothing, until Tsugiki has read this PR from GitHub once.
    pub content: Option<PullRequestContent>,
}

impl PrView {
    /// Reads PR `number`, or nothing if Tsugiki doesn't hold it. This waits on the database and never on
    /// GitHub, so the view opens on what was last read (ADR 0018).
    pub fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        number: u32,
    ) -> Result<Option<Self>, Error> {
        let (facts, content) = reader.pull_request_facts(&repository.name, number)?;
        let causes = Causes::of(&facts, repository.conventions);
        let row = candidates(facts, repository, &BTreeSet::new(), &causes)
            .iter()
            .find_map(|item| {
                item.rows()
                    .into_iter()
                    .find(|row| row.number == number)
                    .cloned()
            });
        Ok(row.map(|row| Self { row, content }))
    }
}

#[cfg(test)]
pub(crate) mod tests;
