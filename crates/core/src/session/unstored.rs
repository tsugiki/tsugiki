//! Fetching an Issue or a PR the database lacks, which the user asked to open by its number (#981). The
//! store saves it as it saves an Origin PR outside the window, and it stays.

use crate::error::{Error, ErrorKind};
use crate::fetching::{Item, ItemFetch, ItemUnfetched};
use crate::github::GitHub;
use crate::github::item::Numbered;
use crate::status::{Change, Status, StatusBoard};
use crate::store::Store;

/// What a fetch saved: which kind the item was, and the change the window hears.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Saved {
    pub kind: ItemFetch,
    pub change: Change,
}

/// Asks GitHub for `item`, Issue or PR, and saves what it found. A failure that speaks for all of GitHub
/// comes back as the error, for the caller to show on the status bar. Every other failure is this item's
/// alone: it is logged, and a database that can't keep the item shows on the status bar too.
pub(super) fn fetch(
    board: &mut StatusBoard,
    store: &Store,
    github: &GitHub,
    item: &Item,
) -> Result<Result<Saved, ItemUnfetched>, Error> {
    let Item { repository, number } = item;
    let fetched = match github.fetch_item(repository, *number) {
        Ok(fetched) => fetched,
        Err(problem) if problem.kind.speaks_for_all_of_github() => return Err(problem),
        Err(problem) => {
            log::warn!(
                "Tsugiki couldn't fetch #{number} of {repository}: {}",
                problem.message
            );
            return Ok(Err(if matches!(problem.kind, ErrorKind::NotFound) {
                ItemUnfetched::Hidden
            } else {
                ItemUnfetched::Other
            }));
        }
    };
    let (saved, kind) = match fetched.numbered {
        Numbered::Issue(facts, activity) => (
            store.save_fetched_issues(repository, vec![facts], vec![activity]),
            ItemFetch::Issue,
        ),
        Numbered::PullRequest(facts, activity) => (
            store.save_fetched_pull_requests(repository, vec![*facts], vec![activity]),
            ItemFetch::PullRequest,
        ),
        Numbered::Missing => return Ok(Err(ItemUnfetched::Missing)),
        Numbered::Hidden => return Ok(Err(ItemUnfetched::Hidden)),
    };
    if let Err(problem) = saved {
        board.set(Status::Problem { problem });
        return Ok(Err(ItemUnfetched::Other));
    }
    let change = Change {
        repository: repository.clone(),
        items: vec![*number],
    };
    Ok(Ok(Saved { kind, change }))
}

#[cfg(test)]
mod tests;
