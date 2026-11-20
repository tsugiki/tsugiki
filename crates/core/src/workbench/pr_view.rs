//! The PR view: a PR's row, and its Conversation (ADR 0008). The Files tab comes with the diff.

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use super::trees::{ItemRead, Named, Trees};
use super::{Hidden, WorkbenchRow, candidates};
use crate::error::Error;
use crate::facts::pull_request_content::PullRequestContent;
use crate::repository_settings::RepositorySettings;
use crate::store::{Snapshot, StoreReader};
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
    /// Reads PR `number`, or nothing if Tsugiki doesn't hold it or nobody has signed in yet. This waits on
    /// the database and never on GitHub, so the view opens on what was last read (ADR 0018). It reads the
    /// trees of the items that hold the PR alone, through the links the store keeps (#842).
    pub(crate) fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        number: u32,
    ) -> Result<Option<Self>, Error> {
        let conventions = repository.conventions;
        let trees_of = move |trees: &mut Trees<'_, '_>| {
            let own = trees.of_prs(&BTreeSet::from([number]))?;
            let issues = trees.issues_holding(&own, number);
            let mut prs = trees.under(&issues)?;
            prs.extend(own);
            Ok(Named { issues, prs })
        };
        let more = move |snapshot: &Snapshot<'_>| snapshot.pull_request_content(number);
        let Some(read) = ItemRead::read(reader, repository, trees_of, more)? else {
            return Ok(None);
        };
        let ItemRead {
            facts,
            links,
            more: content,
        } = read;
        let causes = Causes::of(&facts, conventions);
        let shares_review_of =
            SharedReviews::of(&facts.pull_requests, conventions).origin_of(number);
        let found = candidates((facts, &links), repository, &BTreeSet::new(), &causes)
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
