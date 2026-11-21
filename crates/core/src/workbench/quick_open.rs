//! Quick open: the Issues and PRs of a Repository that hold what the user typed, best first (#557).
//!
//! The user types a number, as `123`, `#123` or `gh-123`, or words from a title. The item with that number
//! comes first. Then come the items whose titles hold every word, in any case: the open ones first, then the
//! most recently updated. Nothing typed lists every item in that order. A number the cache holds no item of
//! is offered for a fetch from GitHub, which only the user's pick of it asks for (#981).

use std::cmp::Reverse;

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::error::Error;
use crate::facts::issues::IssueState;
use crate::facts::pull_requests::PrState;
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
    /// The number the whole text names, when the cache holds no item of it, for the window to offer a
    /// fetch from GitHub (#981).
    pub unstored: Option<u32>,
}

/// The Issues and PRs of `repository` in the cache that `typed` finds, best first, `LISTED` at most in all.
/// This reads the database and never asks GitHub.
pub(crate) fn find(
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
    // GitHub numbers its Issues and PRs from 1, and its numbers are signed 32-bit integers.
    let on_github = |number: &u32| *number > 0 && i32::try_from(*number).is_ok();
    let unstored = typed
        .number
        .filter(|number| named.is_none() && on_github(number));
    Ok(QuickOpenFound {
        named,
        others,
        unstored,
    })
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
