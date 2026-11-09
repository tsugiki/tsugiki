//! Fetching the PRs a sync found, a batch at a time and each once.

use std::collections::BTreeSet;

use super::{Batches, FETCH_BATCH, Link, Moved, Resumable, Step, pause_until};
use crate::error::Error;

pub(super) fn fetch(
    link: &Link<'_>,
    batches: &mut Batches,
    fetched: &mut BTreeSet<u32>,
) -> Result<Moved, Error> {
    let Batches { ids, why, leads } = batches;
    if ids.is_empty() {
        leads.settle_origins(link, fetched)?;
        let next = Resumable::FetchingOrigins {
            why: *why,
            leads: std::mem::take(leads),
        }
        .into();
        return Ok((Step::Moved, Some(next)));
    }
    let batch = link.github.fetch_pull_requests(*why, ids, FETCH_BATCH)?;
    link.store.save_fetched_pull_requests(
        link.repository,
        batch.pull_requests.clone(),
        batch.activity,
    )?;
    for pull_request in &batch.pull_requests {
        leads.learn_from(pull_request, link.repository);
        fetched.extend([pull_request.number]);
    }
    let touched = batch.pull_requests.iter().map(|pr| pr.number).collect();
    drop(ids.drain(..batch.asked_for));
    let pause_until = pause_until(batch.rate_limit);
    let step = Step::Saved {
        touched,
        pause_until,
    };
    Ok((step, None))
}

#[cfg(test)]
mod tests;
