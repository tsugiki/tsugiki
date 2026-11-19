//! The PR view: a PR's row, and its Conversation (ADR 0008). The Files tab comes with the diff.

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use super::{Hidden, WorkbenchRow, candidates_of};
use crate::error::Error;
use crate::facts::pull_request_content::PullRequestContent;
use crate::repository_settings::RepositorySettings;
use crate::store::StoreReader;
use crate::work::{Causes, SharedReviews};

/// A PR as the Detail pane shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PrView {
    #[serde(flatten)]
    pub row: WorkbenchRow,
    /// Its description, comments and Reviews. Nothing, until Tsugiki has read this PR from GitHub once.
    pub content: Option<PullRequestContent>,
    /// The Origin PR whose review state this Bot backport shares (ADR 0011), so its `your_review` names that
    /// PR's reviewed commit and files.
    pub shares_review_of: Option<u32>,
    /// Whether the user's own state hides the item that holds the PR. On a Muted item the header offers no
    /// Mark Seen or Mark Unseen (#492), whether or not the list holds the item.
    pub hidden: Option<Hidden>,
}

impl PrView {
    /// Reads PR `number`, or nothing if Tsugiki doesn't hold it. This waits on the database and never on
    /// GitHub, so the view opens on what was last read (ADR 0018).
    pub(crate) fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        number: u32,
    ) -> Result<Option<Self>, Error> {
        let (facts, content) = reader.pull_request_facts(&repository.name, number)?;
        let causes = Causes::of(&facts, repository.conventions);
        let shares_review_of =
            SharedReviews::of(&facts.pull_requests, repository.conventions).origin_of(number);
        let found = candidates_of(facts, repository, &BTreeSet::new(), &causes)
            .iter()
            .find_map(|item| {
                let row = item.rows().into_iter().find(|row| row.number == number)?;
                let hidden = Hidden::of(&causes, item.key(), &item.numbers());
                Some((row.clone(), hidden))
            });
        Ok(found.map(|(row, hidden)| Self {
            row,
            content,
            shares_review_of,
            hidden,
        }))
    }
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
