//! A first sync's window, after the user's work and before the trees: what changed in the last 30 days,
//! newest first, so All issues is right from its top down while the sync runs (ADR 0015).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::{FETCH_BATCH, Link, Moved, Reading, Resumable, SEARCH_PAGE, Step, pause_until};
use crate::error::Error;
use crate::github::pull_requests::RateLimit;
use crate::github::refresh::ListPlace;
use crate::github::window::WindowRequest;

/// The walk's progress through the window's two lists.
#[derive(Serialize, Deserialize)]
pub(super) struct WindowWalk {
    since: OffsetDateTime,
    issues: WalkedList,
    pull_requests: WalkedList,
    /// The trees to read when the walk ends.
    then: Reading,
}

#[derive(Serialize, Deserialize)]
struct WalkedList {
    place: ListPlace,
    /// The update time of the last item read, which only falls.
    reached: OffsetDateTime,
}

impl WalkedList {
    fn new(top: OffsetDateTime) -> Self {
        Self {
            place: ListPlace::Next(None),
            reached: top,
        }
    }

    fn moved_on(&mut self, next: ListPlace, oldest_read: Option<OffsetDateTime>) {
        self.place = next;
        self.reached = oldest_read.unwrap_or(self.reached);
    }

    /// The point this list has got back to, while it has more to read.
    fn still_at(&self) -> Option<OffsetDateTime> {
        (!self.place.is_finished()).then_some(self.reached)
    }
}

enum NextList {
    Issues,
    PullRequests,
}

impl WindowWalk {
    pub(super) fn new(since: OffsetDateTime, then: Reading) -> Self {
        let top = OffsetDateTime::now_utc();
        Self {
            since,
            issues: WalkedList::new(top),
            pull_requests: WalkedList::new(top),
            then,
        }
    }

    /// The list that has got less far back, so the two lists land newest first between them.
    fn next_list(&self) -> Option<NextList> {
        let issues = self.issues.still_at();
        let pull_requests = self.pull_requests.still_at();
        if pull_requests > issues {
            Some(NextList::PullRequests)
        } else {
            issues.map(|_| NextList::Issues)
        }
    }
}

pub(super) fn walk_window(
    link: &Link<'_>,
    walk: &mut WindowWalk,
    fetched: &mut BTreeSet<u32>,
) -> Result<Moved, Error> {
    let Some(list) = walk.next_list() else {
        let then = std::mem::take(&mut walk.then);
        let next = Resumable::ReadingCrossReferences(then).into();
        return Ok((Step::Moved, Some(next)));
    };
    let (touched, rate_limit) = match list {
        NextList::Issues => read_issues(link, walk)?,
        NextList::PullRequests => read_pull_requests(link, walk, fetched)?,
    };
    Ok((Step::saved_if_any(touched, pause_until(rate_limit)), None))
}

/// One page's numbers, and the points GitHub says are left.
type PageRead = (Vec<u32>, RateLimit);

fn read_issues(link: &Link<'_>, walk: &mut WindowWalk) -> Result<PageRead, Error> {
    let asked = WindowRequest {
        since: walk.since,
        place: &walk.issues.place,
        size: SEARCH_PAGE,
    };
    let page = link.github.window_issues(link.repository, asked)?;
    let oldest_read = page.items.last().map(|issue| issue.updated_at);
    let numbers = page.items.iter().map(|issue| issue.number).collect();
    link.store.save_issues(link.repository, page.items)?;
    walk.issues.moved_on(page.next, oldest_read);
    Ok((numbers, page.rate_limit))
}

fn read_pull_requests(
    link: &Link<'_>,
    walk: &mut WindowWalk,
    fetched: &mut BTreeSet<u32>,
) -> Result<PageRead, Error> {
    let asked = WindowRequest {
        since: walk.since,
        place: &walk.pull_requests.place,
        size: FETCH_BATCH,
    };
    let page = link.github.window_pull_requests(link.repository, asked)?;
    let oldest_read = page.items.last().map(|pr| pr.updated_at);
    let numbers: Vec<u32> = page.items.iter().map(|pr| pr.number).collect();
    link.store.save_pull_requests(link.repository, page.items)?;
    fetched.extend(numbers.iter().copied());
    walk.pull_requests.moved_on(page.next, oldest_read);
    Ok((numbers, page.rate_limit))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use time::{Duration, OffsetDateTime};

    use crate::github::{Login, Reply, Unreachable};
    use crate::repository::RepositorySettings;
    use crate::store::FirstSyncPlace;
    use crate::sync::testing::{
        FEW, Harness, LOTS, cpython, empty_window_page, hours_ago, plain, run_to_the_end,
        searches_finding, window_issue, window_page, window_pr,
    };
    use crate::sync::{Progress, Syncs, UserWorkSync};

    /// The replies up to the window, for a user with no work.
    fn up_to_the_window() -> Vec<Result<Reply, Unreachable>> {
        searches_finding(&[])
    }

    fn window_operations(harness: &Harness) -> Vec<(String, Value)> {
        let asked = harness.asked.try_iter();
        asked
            .filter(|(operation, _)| operation.starts_with("Window"))
            .collect()
    }

    fn run_pages(syncs: &mut Syncs, harness: &Harness, steps: usize) {
        for _ in 0..steps {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }
    }

    #[test]
    fn the_window_lands_newest_first_between_its_two_lists() {
        let mut replies = up_to_the_window();
        replies.push(window_page(
            &[window_issue(1, &hours_ago(1))],
            Some("issues-2"),
            LOTS,
        ));
        replies.push(window_page(
            &[window_pr(2, &hours_ago(2))],
            Some("prs-2"),
            LOTS,
        ));
        replies.push(window_page(&[window_issue(3, &hours_ago(3))], None, LOTS));
        replies.push(window_page(&[window_pr(4, &hours_ago(4))], None, LOTS));
        let harness = Harness::with_window("window-order", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked = window_operations(&harness);
        let cursors: Vec<(&str, &Value)> = asked
            .iter()
            .map(|(operation, sent)| (operation.as_str(), &sent["after"]))
            .collect();
        assert_eq!(
            cursors,
            [
                ("WindowIssues", &Value::Null),
                ("WindowPullRequests", &Value::Null),
                ("WindowIssues", &Value::from("issues-2")),
                ("WindowPullRequests", &Value::from("prs-2")),
            ]
        );
        let facts = harness.reader.work_facts(&cpython()).unwrap();
        let issues: Vec<u32> = facts.issues.iter().map(|issue| issue.number).collect();
        assert_eq!(issues, [1, 3]);
        assert_eq!(harness.numbers(), [2, 4]);
        harness.finish();
    }

    #[test]
    fn a_pr_older_than_the_window_ends_its_list_and_stays_out() {
        let too_old = hours_ago(24 * 31);
        let mut replies = up_to_the_window();
        replies.push(empty_window_page());
        replies.push(window_page(
            &[window_pr(1, &hours_ago(1)), window_pr(2, &too_old)],
            Some("more"),
            LOTS,
        ));
        let harness = Harness::with_window("window-edge", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.numbers(), [1]);
        assert_eq!(window_operations(&harness).len(), 2);
        harness.finish();
    }

    #[test]
    fn the_window_asks_for_the_last_thirty_days_of_issues() {
        let mut replies = up_to_the_window();
        replies.push(empty_window_page());
        replies.push(empty_window_page());
        let harness = Harness::with_window("window-since", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked = window_operations(&harness);
        let since = asked[0].1["since"].as_str().unwrap();
        let since =
            OffsetDateTime::parse(since, &time::format_description::well_known::Rfc3339).unwrap();
        let age = OffsetDateTime::now_utc() - since;
        assert!(age >= Duration::days(30) && age < Duration::days(30) + Duration::minutes(1));
        harness.finish();
    }

    /// Quits after the first page, lets `change` happen, and answers with what the next run asked first.
    fn asked_after_coming_back(name: &str, change: impl Fn(&Harness)) -> Vec<String> {
        let harness = Harness::with_window(name, searches_finding(&[]));
        let mut quitting = Syncs::default();
        quitting.add(plain(cpython()));
        drop(quitting.step(&harness.github, &harness.store).unwrap());
        change(&harness);

        let mut back = Syncs::default();
        back.add(plain(cpython()));
        drop(back.step(&harness.github, &harness.store).unwrap());

        let operations = harness.operations();
        harness.finish();
        operations
    }

    const STARTED_OVER: [&str; 2] = ["SearchReviewRequests", "SearchReviewRequests"];

    #[test]
    fn a_place_goes_on_when_nothing_has_changed() {
        let asked = asked_after_coming_back("window-same", |_| {});
        assert_eq!(asked, ["SearchReviewRequests", "SearchReviewedPrs"]);
    }

    #[test]
    fn signing_in_as_someone_else_starts_the_sync_over() {
        let asked = asked_after_coming_back("window-account", |harness| {
            let other = Login::new("another-account");
            harness.store.remember_login(&other).unwrap();
        });
        assert_eq!(asked, STARTED_OVER);
    }

    #[test]
    fn a_place_from_another_version_starts_the_sync_over() {
        let asked = asked_after_coming_back("window-version", |harness| {
            let place = harness.store.first_sync_place(&cpython()).unwrap().unwrap();
            let version = env!("CARGO_PKG_VERSION");
            let older = place.as_str().replace(version, "0.0.0-older");
            assert_ne!(older, place.as_str());
            harness
                .store
                .save_first_sync_place(&cpython(), FirstSyncPlace::new(older))
                .unwrap();
        });
        assert_eq!(asked, STARTED_OVER);
    }

    #[test]
    fn editing_the_filters_starts_the_sync_over() {
        let harness = Harness::with_window("window-filters", searches_finding(&[]));
        let mut quitting = Syncs::default();
        quitting.add(plain(cpython()));
        drop(quitting.step(&harness.github, &harness.store).unwrap());

        let mut back = Syncs::default();
        back.add(RepositorySettings::new(cpython()));
        drop(back.step(&harness.github, &harness.store).unwrap());

        assert_eq!(harness.operations(), STARTED_OVER);
        harness.finish();
    }

    #[test]
    fn the_window_tells_a_screen_of_each_page_that_saved_something() {
        let mut replies = up_to_the_window();
        replies.push(window_page(
            &[window_issue(1, &hours_ago(1))],
            Some("more"),
            LOTS,
        ));
        replies.push(window_page(&[window_pr(2, &hours_ago(2))], None, LOTS));
        replies.push(window_page(&[window_issue(3, &hours_ago(3))], None, LOTS));
        let harness = Harness::with_window("window-news", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        let mut told = Vec::new();
        while syncs.ready_at().is_some() {
            if let Some(Progress::Changed(change)) =
                syncs.step(&harness.github, &harness.store).unwrap()
            {
                told.push(change.items);
            }
        }

        assert_eq!(told, [[1], [2], [3]]);
        harness.finish();
    }

    #[test]
    fn a_window_page_below_the_floor_holds_the_next_step_back() {
        let mut replies = up_to_the_window();
        replies.push(window_page(
            &[window_issue(1, &hours_ago(1))],
            Some("more"),
            FEW,
        ));
        let harness = Harness::with_window("window-floor", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = std::time::Instant::now();

        // Four searches, five moves and the page.
        run_pages(&mut syncs, &harness, 10);

        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + std::time::Duration::from_secs(3000));
        harness.finish();
    }

    #[test]
    fn a_first_sync_taken_up_again_keeps_when_it_started() {
        let long_ago = OffsetDateTime::now_utc() - Duration::days(2);
        let harness = Harness::with_window("window-started", up_to_the_window());
        let mut first = UserWorkSync::new(plain(cpython()));
        first.started_at = long_ago;
        drop(first.step(&harness.github, &harness.store).unwrap());

        let mut taken_up = UserWorkSync::new(plain(cpython()));
        drop(taken_up.step(&harness.github, &harness.store).unwrap());

        assert_eq!(taken_up.started_at, long_ago);
        assert_eq!(taken_up.first_sync.map(|sync| sync.pages), Some(2));
        harness.finish();
    }

    #[test]
    fn a_finished_sync_has_no_place_to_come_back_to() {
        let mut replies = up_to_the_window();
        replies.push(empty_window_page());
        replies.push(empty_window_page());
        let harness = Harness::with_window("window-finished", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        run_pages(&mut syncs, &harness, 1);
        assert!(
            harness
                .store
                .first_sync_place(&cpython())
                .unwrap()
                .is_some()
        );

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.store.first_sync_place(&cpython()).unwrap(), None);
        harness.finish();
    }

    #[test]
    fn a_place_it_cant_read_starts_the_sync_over() {
        let mut replies = up_to_the_window();
        replies.push(empty_window_page());
        replies.push(empty_window_page());
        let harness = Harness::with_window("window-unreadable", replies);
        harness
            .store
            .save_first_sync_place(&cpython(), FirstSyncPlace::new("{\"from\":0}".to_owned()))
            .unwrap();
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let operations = harness.operations();
        assert_eq!(operations[0], "SearchReviewRequests");
        assert!(harness.store.sync_state(&cpython()).unwrap().is_some());
        harness.finish();
    }

    #[test]
    fn a_step_whose_place_cant_be_saved_keeps_its_news_for_the_next_try() {
        let mut replies = up_to_the_window();
        replies.push(window_page(&[window_issue(1, &hours_ago(1))], None, LOTS));
        let harness = Harness::with_window("window-unsaved", replies);
        let mut sync = UserWorkSync::new(plain(cpython()));
        // Four searches and five moves leave the page to read.
        for _ in 0..9 {
            drop(sync.step(&harness.github, &harness.store).unwrap());
        }
        // This page's save lands, but saving its place doesn't.
        let (saves_once, _calls) = harness.store.failing_at_call(Some(1));

        let failed = sync.step(&harness.github, &saves_once);
        let again = sync.step(&harness.github, &saves_once).unwrap();

        saves_once.close();
        assert!(failed.is_err());
        assert!(matches!(again, crate::sync::Step::Saved { .. }));
        assert_eq!(harness.operations().len(), 5);
        harness.finish();
    }
}
