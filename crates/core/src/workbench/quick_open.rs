//! Quick open: the Issues and PRs of a Repository that hold what the user typed, best first (#557).
//!
//! The user types a number, as `123`, `#123` or `gh-123`, or words from a title. The item with that number
//! comes first. Then come the items whose titles hold every word, in any case: the open ones first, then the
//! most recently updated. Nothing typed lists every item in that order.

use std::cmp::Reverse;

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::error::Error;
use crate::github::issues::IssueState;
use crate::github::pull_requests::PrState;
use crate::repository::RepositoryName;
use crate::store::{StoreReader, TitledItem, TitledKind};

/// How many items quick open lists at most: enough to pick from, and few enough to fit the window.
pub const LISTED: usize = 20;

/// What quick open finds for a text: the item the text names by its number, apart, so the window can put it
/// ahead of the files of the PR on show (#558), and the other items.
#[derive(Debug, PartialEq, Eq, Serialize, Type)]
pub struct QuickOpenFound {
    /// The Issue or PR whose number the whole text names, if the cache holds it.
    pub named: Option<TitledItem>,
    /// The items whose titles hold every word typed: the open first, then the newest.
    pub others: Vec<TitledItem>,
}

/// The Issues and PRs of `repository` in the cache that `typed` finds, best first, `LISTED` at most in all.
/// This reads the database and never asks GitHub.
pub fn find(
    reader: &StoreReader,
    repository: &RepositoryName,
    typed: &str,
) -> Result<QuickOpenFound, Error> {
    let typed = Typed::parse(typed);
    let mut found: Vec<TitledItem> = reader
        .titled_items(repository)?
        .into_iter()
        .filter(|item| typed.finds(item))
        .collect();
    let named = typed
        .number
        .and_then(|number| found.iter().position(|item| item.number == number))
        .map(|at| found.swap_remove(at));
    let others = best(found, LISTED - usize::from(named.is_some()));
    Ok(QuickOpenFound { named, others })
}

/// What the user typed, read for the search.
#[derive(Debug, PartialEq, Eq)]
struct Typed {
    /// The number the whole text names, if it names one.
    number: Option<u32>,
    /// Each word of the text, lowercased, which a title has to hold.
    words: Vec<String>,
}

impl Typed {
    fn parse(typed: &str) -> Self {
        let text = typed.trim();
        let digits = text
            .strip_prefix('#')
            .or_else(|| {
                text.get(..3)
                    .filter(|prefix| prefix.eq_ignore_ascii_case("gh-"))
                    .and_then(|_| text.get(3..))
            })
            .unwrap_or(text);
        let number = (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| digits.parse().ok())
            .flatten();
        Self {
            number,
            words: text.split_whitespace().map(str::to_lowercase).collect(),
        }
    }

    /// Whether `item` is the one the number names, or its title holds every word. Nothing typed finds every
    /// item.
    fn finds(&self, item: &TitledItem) -> bool {
        if self.words.is_empty() || self.number == Some(item.number) {
            return true;
        }
        let title = item.title.to_lowercase();
        self.words.iter().all(|word| title.contains(word.as_str()))
    }
}

/// Where `item` comes in the list, lowest first: the open ones, the newest first.
fn rank(item: &TitledItem) -> (bool, Reverse<OffsetDateTime>, Reverse<u32>) {
    (
        !is_open(item.kind),
        Reverse(item.updated_at),
        Reverse(item.number),
    )
}

/// The best of `found`, `listed` at most, best first. Only those `listed` get sorted.
fn best(mut found: Vec<TitledItem>, listed: usize) -> Vec<TitledItem> {
    if found.len() > listed {
        let (best, _, _) = found.select_nth_unstable_by_key(listed, rank);
        best.sort_unstable_by_key(rank);
        found.truncate(listed);
    } else {
        found.sort_unstable_by_key(rank);
    }
    found
}

fn is_open(kind: TitledKind) -> bool {
    match kind {
        TitledKind::Issue { state } => state == Some(IssueState::Open),
        TitledKind::PullRequest { state, .. } => state == PrState::Open,
    }
}

#[cfg(test)]
mod tests;
