//! Fetching the Issues a sync's PRs name, a batch at a time.

use super::loading::Loading;

use super::{FETCH_BATCH, Link, Moved, Reading, Resumable, Step};
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
    // A number GitHub has no Issue under, a PR's, a transferred or a deleted Issue's, keeps no pin, so no
    // sync asks about it again (#488). One GitHub refused alone waits for the next sync.
    let not_found = |number: &&u32| {
        !found.contains(number)
            && !batch.issueless.contains(number)
            && !batch.refused.contains(number)
    };
    let asked = numbers.iter().take(batch.asked_for);
    let gone: Vec<u32> = asked.filter(not_found).copied().collect();
    let issueless = batch.issueless;
    link.store.save_issues_and_issueless(
        link.repository,
        (batch.issues, batch.activity),
        issueless.clone(),
    )?;
    if !gone.is_empty() {
        link.store.drop_pins(link.repository, gone.clone())?;
    }
    shown.extend(found.iter().copied());
    drop(numbers.drain(..batch.asked_for));
    let touched = found.into_iter().chain(issueless).chain(gone).collect();
    Ok((Step::Saved { touched }, None))
}

/// Fetches the Issues a tree's PRs name, for their rows only, and then the tree is done.
pub(super) fn fetch_named_issues(link: &Link<'_>, numbers: &mut Vec<u32>) -> Result<Moved, Error> {
    if numbers.is_empty() {
        return Ok((Step::Finished, None));
    }
    let saved = fetch_issue_batch(link, numbers)?;
    Ok((Step::saved_if_any(saved.touched), None))
}

/// The Issues of one batch, fetched and saved.
pub(super) struct SavedIssues {
    /// The count of numbers, from the front of those asked about, that the batch took.
    pub(super) asked_for: usize,
    /// The Issues saved. An Issue GitHub no longer has stays as it is stored, and isn't among them.
    pub(super) touched: Vec<u32>,
}

/// Fetches the first batch of `numbers`, which holds at least one, saves the Issues and takes the batch off
/// `numbers`.
pub(super) fn fetch_issue_batch(
    link: &Link<'_>,
    numbers: &mut Vec<u32>,
) -> Result<SavedIssues, Error> {
    let saved = save_issue_batch(link, numbers)?;
    drop(numbers.drain(..saved.asked_for));
    Ok(saved)
}

/// Fetches the first batch of `numbers`, which holds at least one, and saves the Issues, leaving `numbers`
/// as they are.
pub(super) fn save_issue_batch(link: &Link<'_>, numbers: &[u32]) -> Result<SavedIssues, Error> {
    let batch = link
        .github
        .fetch_issues(link.repository, numbers, FETCH_BATCH)?;
    let touched = batch.issues.iter().map(|issue| issue.number).collect();
    link.store
        .save_fetched_issues(link.repository, batch.issues, batch.activity)?;
    Ok(SavedIssues {
        asked_for: batch.asked_for,
        touched,
    })
}

#[cfg(test)]
mod tests;
