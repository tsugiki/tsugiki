//! Fetching the Issues a sync's PRs name, a batch at a time.

use super::{FETCH_BATCH, Link, Moved, NO_PAUSE, Phase, Reading, Step, pause_until};
use crate::error::Error;

pub(super) fn fetch_issues(
    link: &Link<'_>,
    numbers: &mut Vec<u32>,
    shown: &mut Vec<u32>,
) -> Result<Moved, Error> {
    if numbers.is_empty() {
        let next = Phase::ReadingCrossReferences(Reading {
            issues: std::mem::take(shown),
            cursor: None,
            listed: Vec::new(),
        });
        return Ok((NO_PAUSE, Some(next)));
    }
    let batch = link
        .github
        .fetch_issues(link.repository, numbers, FETCH_BATCH)?;
    let numbered = batch.issues.iter().map(|issue| issue.number);
    let found: Vec<u32> = numbered.collect();
    link.store.save_issues(link.repository, batch.issues)?;
    shown.extend(found.iter().copied());
    drop(numbers.drain(..batch.asked_for));
    let pause_until = pause_until(batch.rate_limit);
    let step = Step::Saved {
        touched: found,
        pause_until,
    };
    Ok((step, None))
}

#[cfg(test)]
mod tests {
    use crate::github::{Reply, Unreachable};
    use crate::repository::RepositoryName;
    use crate::sync::Syncs;
    use crate::sync::testing::{
        FEW, Harness, LOTS, cpython, fetched, fetched_titled, issues, no_cross_references, plain,
        run_to_the_end, searches_finding,
    };
    use std::time::{Duration, Instant};

    /// An answer to a fetch of PRs, each of whose titles names the Issue with the same number.
    fn fetched_naming_issues(numbers: &[u32], rate: &str) -> Result<Reply, Unreachable> {
        fetched_titled(numbers, rate, |number| format!("gh-{number}: Fix it"))
    }

    #[test]
    fn a_sync_fetches_the_issues_its_prs_name_after_the_prs() {
        let mut replies = searches_finding(&["a", "b"]);
        replies.push(fetched_naming_issues(&[1, 2], LOTS));
        replies.push(issues(&[1, 2], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("issues", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        let changed = run_to_the_end(&mut syncs, &harness);

        let asked: Vec<String> = harness.operations();
        assert_eq!(
            asked[4..],
            [
                "FetchPullRequests",
                "FetchIssues",
                "FetchCrossReferences",
                "FetchCrossReferences",
            ]
        );
        let facts = harness.reader.work_facts(&cpython()).unwrap();
        let stored: Vec<u32> = facts.issues.iter().map(|issue| issue.number).collect();
        assert_eq!(stored, [1, 2]);
        assert_eq!(changed, [cpython(), cpython()]);
        harness.finish();
    }

    #[test]
    fn a_sync_with_no_named_issues_asks_for_none() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched(&[1], LOTS));
        let harness = Harness::new("no-issues", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.operations().len(), 5);
        harness.finish();
    }

    #[test]
    fn a_sync_fetches_a_titles_issue_whatever_the_conventions_of_its_repository() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_naming_issues(&[1], LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("conventions", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            harness.operations()[4..],
            ["FetchPullRequests", "FetchIssues", "FetchCrossReferences"]
        );
        harness.finish();
    }

    #[test]
    fn a_fetch_of_issues_goes_batch_by_batch_and_pauses_below_the_floor() {
        let ids: Vec<u32> = (1..=30).collect();
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_naming_issues(&ids, LOTS));
        replies.push(issues(&[1], FEW));
        replies.push(issues(&[2], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("issue-batches", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = Instant::now();

        // Four searches, two moves, the PRs, two moves and the first batch of Issues.
        for _ in 0..10 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }
        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
        // The two batches take all 30 numbers.
        drop(run_to_the_end(&mut syncs, &harness));
        assert_eq!(syncs.ready_at(), None);

        let facts = harness.reader.work_facts(&cpython()).unwrap();
        assert_eq!(facts.issues.len(), 2);
        harness.finish();
    }
}
