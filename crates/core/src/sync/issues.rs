//! Fetching the Issues a sync's PRs name, a batch at a time.

use super::loading::Loading;
use time::OffsetDateTime;

use super::{FETCH_BATCH, Link, Moved, Reading, Resumable, Step, pause_until};
use crate::error::Error;

pub(super) fn fetch_issues(
    link: &Link<'_>,
    numbers: &mut Vec<u32>,
    shown: &mut Vec<u32>,
) -> Result<Moved, Error> {
    if numbers.is_empty() {
        let issues = std::mem::take(shown);
        let next = match link.window_since {
            Some(since) => Resumable::Loading(Loading::new(since, issues)),
            None => Resumable::ReadingCrossReferences(Reading {
                issues,
                cursor: None,
                listed: Vec::new(),
            }),
        };
        return Ok((Step::Moved, Some(next.into())));
    }
    let batch = link
        .github
        .fetch_issues(link.repository, numbers, FETCH_BATCH)?;
    let numbered = batch.issues.iter().map(|issue| issue.number);
    let found: Vec<u32> = numbered.collect();
    link.store
        .save_fetched_issues(link.repository, batch.issues, batch.activity)?;
    shown.extend(found.iter().copied());
    drop(numbers.drain(..batch.asked_for));
    let pause_until = pause_until(batch.rate_limit);
    let step = Step::Saved {
        touched: found,
        pause_until,
    };
    Ok((step, None))
}

/// Fetches the Issues a tree's PRs name, for their rows only, and then the tree is done.
pub(super) fn fetch_named_issues(link: &Link<'_>, numbers: &mut Vec<u32>) -> Result<Moved, Error> {
    if numbers.is_empty() {
        return Ok((Step::Finished, None));
    }
    let (touched, pause_until) = fetch_issue_batch(link, numbers)?;
    Ok((Step::saved_if_any(touched, pause_until), None))
}

/// Fetches the first batch of `numbers`, which holds at least one, saves the Issues and takes the batch off
/// `numbers`. It answers with the Issues it saved and when to pause. An Issue GitHub no longer has stays as
/// it is stored.
pub(super) fn fetch_issue_batch(
    link: &Link<'_>,
    numbers: &mut Vec<u32>,
) -> Result<(Vec<u32>, Option<OffsetDateTime>), Error> {
    let batch = link
        .github
        .fetch_issues(link.repository, numbers, FETCH_BATCH)?;
    let touched = batch.issues.iter().map(|issue| issue.number).collect();
    link.store
        .save_fetched_issues(link.repository, batch.issues, batch.activity)?;
    drop(numbers.drain(..batch.asked_for));
    Ok((touched, pause_until(batch.rate_limit)))
}

#[cfg(test)]
mod tests;
