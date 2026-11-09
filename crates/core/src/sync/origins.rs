//! Fetching the Origin PR a Backport names, when Tsugiki lacks it (ADR 0011).

use std::collections::BTreeSet;

use super::{Leads, Link, Moved, Phase, Resumable, Step, pause_until};
use crate::error::Error;
use crate::github::pull_requests::PrFetch;
use crate::links::possible_numbers;

/// The next phase if a tree's PRs name Issues Tsugiki lacks. With no Issue, such a PR would show as a
/// Standalone PR, however old it is. A tree goes one level, so those Issues come without trees of their own
/// (ADR 0011).
fn issues_a_tree_lacks(link: &Link<'_>, leads: &mut Leads) -> Result<Option<Phase>, Error> {
    if leads.named.is_empty() {
        return Ok(None);
    }
    let stored = link.store.issues(link.repository)?;
    let held: BTreeSet<u32> = stored.iter().map(|issue| issue.number).collect();
    let lacking: Vec<u32> = leads.named.difference(&held).copied().collect();
    Ok((!lacking.is_empty()).then(|| Resumable::FetchingNamedIssues(lacking).into()))
}

/// The next Origin PR to ask for, one at a time.
pub(super) fn fetch_origin(
    link: &Link<'_>,
    why: PrFetch,
    leads: &mut Leads,
    fetched: &mut BTreeSet<u32>,
) -> Result<Moved, Error> {
    let Some(number) = leads.origins.first().copied() else {
        let next = match why {
            PrFetch::Work => {
                let named = std::mem::take(&mut leads.named);
                let held = std::mem::take(&mut leads.held_issues);
                Some(
                    Resumable::FetchingIssues {
                        shown: held.difference(&named).copied().collect(),
                        numbers: named.into_iter().collect(),
                    }
                    .into(),
                )
            }
            PrFetch::Tree | PrFetch::Changed => issues_a_tree_lacks(link, leads)?,
        };
        let step = if next.is_some() {
            Step::Moved
        } else {
            Step::Finished
        };
        return Ok((step, next));
    };
    let answer = link
        .github
        .fetch_origin_pull_request(link.repository, number)?;
    let pause_until = pause_until(answer.rate_limit);
    let step = if let Some((pull_request, activity)) = answer.pull_request {
        let named = possible_numbers(&pull_request, link.repository);
        link.store.save_fetched_pull_requests(
            link.repository,
            vec![pull_request],
            vec![activity],
        )?;
        leads.named.extend(named);
        Step::Saved {
            touched: vec![number],
            pause_until,
        }
    } else {
        Step::Searched { pause_until }
    };
    fetched.extend([number]);
    leads.origins.retain(|other| *other != number);
    Ok((step, None))
}

#[cfg(test)]
mod tests;
