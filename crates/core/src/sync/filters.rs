//! A sync's walk through the Filters' searches (ADR 0015). It reads each search whole, or through ranges of
//! days when GitHub won't list it whole, and saves what the search found. It then asks for what Tsugiki
//! lacks. A refresh narrows a search to what changed, and merges the answer.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use time::{Date, OffsetDateTime};

use super::{Batches, Leads, Link, Moved, NO_PAUSE, Phase, SEARCH_PAGE, Step, pause_until};
use crate::error::{Error, ErrorKind};
use crate::filters::FilterQuery;
use crate::github::filters::{CreatedRange, FilterHit, FilterHitKind, FilterSearch};
use crate::github::pull_requests::{NodeId, PageRequest, PrFetch, PullRequestFacts};
use crate::repository::RepositoryName;
use crate::store::{FilterProblem, FilterResults, NarrowedResults};

/// The most results GitHub lists for one search (ADR 0015).
const SEARCH_CAP: u32 = 1000;

/// One search of a walk.
#[derive(Clone)]
struct WalkedSearch {
    query: FilterQuery,
    /// Keeps only what changed since this time. Without it, the search replaces the Filter's results.
    since: Option<OffsetDateTime>,
}

/// The Filters' searches, one at a time, with what they've found.
#[derive(Default)]
pub(super) struct FilterWalk {
    /// The searches still to finish. The first is the one under way.
    left: VecDeque<WalkedSearch>,
    /// The parts of that search still to read. The last is the one under way.
    slices: Vec<Slice>,
    cursor: Option<String>,
    /// That search's results so far.
    current: BTreeSet<u32>,
    cut_off: bool,
    /// The items all the searches have found.
    hits: BTreeMap<u32, FilterHit>,
    /// The PRs the user's own searches found, which a sync always fetches.
    work: Vec<NodeId>,
    /// What a refresh found before this walk began.
    leads: Leads,
    /// Each item a refresh found updated, which a narrowed search can say has stopped matching.
    changed: BTreeSet<u32>,
}

/// What a refresh hands to its walk.
#[derive(Default)]
pub(super) struct RefreshWalk {
    /// The searches to run whole.
    pub(super) whole: Vec<FilterQuery>,
    /// The searches to narrow to `since`.
    pub(super) narrowed: Vec<FilterQuery>,
    pub(super) since: Option<OffsetDateTime>,
    pub(super) changed: BTreeSet<u32>,
    /// What the refresh's poll found for the searches that don't walk.
    pub(super) hits: BTreeMap<u32, FilterHit>,
}

/// True when a refresh can narrow this search: it has finished once, and GitHub didn't refuse it last time.
/// A search GitHub cuts off stays narrowed, because a whole read would hit the same limit.
pub(super) fn can_narrow(stored: Option<&FilterResults>) -> bool {
    stored.is_some_and(|results| !matches!(results.problem, Some(FilterProblem::Refused(_))))
}

/// A part of a search GitHub can list whole.
#[derive(Clone, Copy)]
enum Slice {
    Whole,
    Created(CreatedRange),
}

impl Slice {
    fn created(self) -> Option<CreatedRange> {
        match self {
            Self::Whole => None,
            Self::Created(range) => Some(range),
        }
    }

    /// This part in two, the older first. The search has to end somewhere, and a single day can't be cut.
    fn halves(self, today: Date) -> Option<(Self, Self)> {
        let range = self.created().unwrap_or_else(|| CreatedRange::up_to(today));
        let (older, newer) = range.halves()?;
        Some((Self::Created(older), Self::Created(newer)))
    }
}

impl FilterWalk {
    pub(super) fn new(queries: &[FilterQuery], work: Vec<NodeId>) -> Self {
        Self {
            left: queries
                .iter()
                .cloned()
                .map(|query| WalkedSearch { query, since: None })
                .collect(),
            slices: vec![Slice::Whole],
            work,
            ..Self::default()
        }
    }

    /// The walk at the end of a refresh. The searches to run whole go first, as they have nothing to show.
    pub(super) fn of_refresh(plan: RefreshWalk) -> Self {
        let RefreshWalk {
            whole,
            narrowed,
            since,
            changed,
            hits,
        } = plan;
        let whole = whole
            .into_iter()
            .map(|query| WalkedSearch { query, since: None });
        let narrowed = narrowed
            .into_iter()
            .map(|query| WalkedSearch { query, since });
        Self {
            left: whole.chain(narrowed).collect(),
            slices: vec![Slice::Whole],
            hits,
            changed,
            ..Self::default()
        }
    }

    /// The Issues a refresh changed, whose trees this sync reads.
    pub(super) fn hold_issues(&mut self, issues: BTreeSet<u32>) {
        self.leads.held_issues.extend(issues);
    }

    /// Follows what a refresh's fetch of the user's own PR points at.
    pub(super) fn learn_from(
        &mut self,
        pull_request: &PullRequestFacts,
        repository: &RepositoryName,
    ) {
        self.leads.learn_from(pull_request, repository);
    }
}

/// Reads the next page of the search under way, or moves on from the last of them. One Filter that GitHub
/// won't answer keeps what it last found and says why, and stops nothing else (ADR 0015).
pub(super) fn walk_filters(link: &Link<'_>, walk: &mut FilterWalk) -> Result<Moved, Error> {
    let (Some(search), Some(slice)) = (walk.left.front().cloned(), walk.slices.last().copied())
    else {
        return Ok((NO_PAUSE, Some(fetch_what_is_missing(link, walk)?)));
    };
    let query = &search.query;
    let asked = FilterSearch {
        query,
        created: slice.created(),
        since: search.since,
        page: PageRequest {
            size: SEARCH_PAGE,
            after: walk.cursor.as_deref(),
        },
    };
    let page = match link.github.search_filter(link.repository, &asked) {
        Ok(page) => page,
        Err(problem) if blames_the_search(&problem) => {
            log::warn!("GitHub wouldn't search {query:?}: {}", problem.message);
            link.store
                .save_filter_problem(link.repository, query, &problem.message)?;
            walk.start_the_next_search();
            let saved = Step::Saved {
                touched: Vec::new(),
                pause_until: None,
            };
            return Ok((saved, None));
        }
        Err(problem) => return Err(problem),
    };
    let pause_until = pause_until(page.rate_limit);
    if walk.cursor.is_none() && page.total > SEARCH_CAP {
        let today = OffsetDateTime::now_utc().date();
        if let Some((older, newer)) = slice.halves(today) {
            let _halved = walk.slices.pop();
            walk.slices.extend([older, newer]);
            return Ok((Step::Searched { pause_until }, None));
        }
        walk.cut_off = true;
    }
    for hit in page.hits {
        walk.current.extend([hit.number]);
        drop(walk.hits.insert(hit.number, hit));
    }
    walk.cursor = page.next;
    if walk.cursor.is_none() {
        let _read = walk.slices.pop();
    }
    if !walk.slices.is_empty() {
        return Ok((Step::Searched { pause_until }, None));
    }
    let numbers = std::mem::take(&mut walk.current);
    let step = if search.since.is_some() {
        let narrowed = NarrowedResults {
            changed: walk.changed.clone(),
            found: numbers,
            cut_off: walk.cut_off,
        };
        let moved = link
            .store
            .merge_filter_results(link.repository, query, narrowed)?;
        let touched: Vec<u32> = moved.into_iter().collect();
        // The Sidebar shows the problem, so a cut-off search counts as a change even with no number in it.
        if walk.cut_off {
            Step::Saved {
                touched,
                pause_until,
            }
        } else {
            Step::saved_if_any(touched, pause_until)
        }
    } else {
        let touched = numbers.iter().copied().collect();
        let problem = walk.cut_off.then_some(FilterProblem::CutOff);
        let results = FilterResults { numbers, problem };
        link.store
            .save_filter_results(link.repository, query, results)?;
        Step::Saved {
            touched,
            pause_until,
        }
    };
    walk.start_the_next_search();
    Ok((step, None))
}

/// True when GitHub refused this search, which the next Filter's search wouldn't run into. An outage, no
/// network, a refused token or no points left would stop that search too, so the sync tries this one again.
pub(super) fn blames_the_search(problem: &Error) -> bool {
    match problem.kind {
        ErrorKind::NotFound | ErrorKind::QueryRefused => true,
        ErrorKind::Offline
        | ErrorKind::Unauthorized
        | ErrorKind::WriteRefused
        | ErrorKind::RateLimited { .. }
        | ErrorKind::Conflict
        | ErrorKind::Internal => false,
    }
}

impl FilterWalk {
    fn start_the_next_search(&mut self) {
        drop(self.left.pop_front());
        self.slices = vec![Slice::Whole];
        self.cursor = None;
        self.current.clear();
        self.cut_off = false;
    }
}

/// The user's own PRs, and each item a Filter found that Tsugiki lacks or has an older copy of. An item it
/// has as it is now only passes on what it points at, and so does a merged PR (ADR 0011). An open PR left out
/// here still comes with its Issue's tree, which keeps its CI current: CI finishing doesn't change
/// `updatedAt`.
fn fetch_what_is_missing(link: &Link<'_>, walk: &mut FilterWalk) -> Result<Phase, Error> {
    let stored = link.store.pull_requests(link.repository)?;
    let stored: BTreeMap<u32, PullRequestFacts> =
        stored.into_iter().map(|pr| (pr.number, pr)).collect();
    let issues = link.store.issues(link.repository)?;
    let issues: BTreeMap<u32, OffsetDateTime> = issues
        .iter()
        .map(|issue| (issue.number, issue.updated_at))
        .collect();
    let mut ids = std::mem::take(&mut walk.work);
    let mut asking: BTreeSet<NodeId> = ids.iter().cloned().collect();
    let mut leads = std::mem::take(&mut walk.leads);
    for hit in std::mem::take(&mut walk.hits).into_values() {
        match hit.kind {
            FilterHitKind::Issue => {
                if issues
                    .get(&hit.number)
                    .is_some_and(|at| *at >= hit.updated_at)
                {
                    leads.held_issues.extend([hit.number]);
                } else {
                    leads.named.extend([hit.number]);
                }
            }
            FilterHitKind::PullRequest(id) => match stored.get(&hit.number) {
                Some(pr) if pr.stays_as_it_is() || pr.updated_at >= hit.updated_at => {
                    leads.learn_from(pr, link.repository);
                }
                Some(_) | None => {
                    if asking.insert(id.clone()) {
                        ids.push(id);
                    }
                }
            },
        }
    }
    Ok(Phase::Fetching(Batches {
        ids,
        why: PrFetch::Work,
        leads,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::Filter;
    use crate::github::testing::reply_ok;
    use crate::github::{Reply, Request, Transport, Unreachable};
    use crate::repository::RepositorySettings;
    use crate::sync::Syncs;
    use crate::sync::tests::{
        FEW, Harness, LOTS, cpython, cross_references, fetched, fetched_nodes, issues,
        no_cross_references, nothing, pr_node, run_to_the_end, searches_finding,
    };
    use crate::token::Token;
    use serde_json::{Value, json};
    use std::time::{Duration, Instant};

    /// A Repository with these searches as Filters.
    fn filtering(queries: &[&str]) -> RepositorySettings {
        RepositorySettings {
            filters: queries
                .iter()
                .map(|text| Filter {
                    name: format!("Named {text}").try_into().unwrap(),
                    query: filter_query(text),
                })
                .collect(),
            ..RepositorySettings::new(cpython())
        }
    }

    fn filter_query(text: &str) -> FilterQuery {
        text.to_owned().try_into().unwrap()
    }

    fn issue_hit(number: u32) -> Value {
        json!({
            "__typename": "Issue",
            "number": number,
            "updatedAt": "2026-11-04T10:00:00Z",
            "repository": { "nameWithOwner": "python/cpython" },
        })
    }

    /// A PR the search says it last saw at `updated_at`.
    fn pr_hit(number: u32, updated_at: &str) -> Value {
        json!({
            "__typename": "PullRequest",
            "id": format!("id{number}"),
            "number": number,
            "updatedAt": updated_at,
            "repository": { "nameWithOwner": "python/cpython" },
        })
    }

    fn filter_page(total: u32, nodes: &[Value], next: Option<&str>) -> Result<Reply, Unreachable> {
        let info = json!({ "hasNextPage": next.is_some(), "endCursor": next });
        let nodes = json!(nodes);
        reply_ok(&format!(
            r#"{{"data":{{"search":{{"issueCount":{total},"pageInfo":{info},"nodes":{nodes}}},{LOTS}}}}}"#
        ))
    }

    fn all_of(nodes: &[Value]) -> Result<Reply, Unreachable> {
        let total = nodes.iter().fold(0, |total, _| total + 1);
        filter_page(total, nodes, None)
    }

    fn found_by(harness: &Harness, query: &str) -> Vec<u32> {
        let facts = harness.reader.work_facts(&cpython()).unwrap();
        facts
            .filters
            .get(&filter_query(query))
            .map(|results| results.numbers.iter().copied().collect())
            .unwrap_or_default()
    }

    fn filter_problem(harness: &Harness, query: &str) -> Option<String> {
        let facts = harness.reader.work_facts(&cpython()).unwrap();
        facts
            .filters
            .get(&filter_query(query))
            .and_then(|results| results.problem.clone())
            .map(|problem| problem.words().to_owned())
    }

    #[test]
    fn a_filter_runs_after_the_users_work_and_fetches_what_it_found() {
        let mut replies = searches_finding(&[]);
        replies.push(all_of(&[issue_hit(1), pr_hit(10, "2026-11-04T10:00:00Z")]));
        replies.push(fetched(&[10], LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("filter-first", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            harness.operations()[4..],
            [
                "SearchFilter",
                "FetchPullRequests",
                "FetchIssues",
                "FetchCrossReferences"
            ]
        );
        assert_eq!(found_by(&harness, "a"), [1, 10]);
        assert_eq!(harness.numbers(), [10]);
        harness.finish();
    }

    #[test]
    fn a_filters_search_says_the_repository_and_follows_its_pages() {
        let mut replies = searches_finding(&[]);
        replies.push(filter_page(2, &[issue_hit(1)], Some("more")));
        replies.push(filter_page(2, &[issue_hit(2)], None));
        replies.push(issues(&[1, 2], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("filter-pages", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["label:bug"]));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[4].1["query"], "repo:python/cpython (label:bug)");
        assert_eq!(asked[4].1["after"], Value::Null);
        assert_eq!(asked[5].1["after"], "more");
        assert_eq!(found_by(&harness, "label:bug"), [1, 2]);
        harness.finish();
    }

    #[test]
    fn each_filter_keeps_its_own_results_and_a_shared_item_is_fetched_once() {
        let mut replies = searches_finding(&[]);
        replies.push(all_of(&[issue_hit(1), issue_hit(2)]));
        replies.push(all_of(&[issue_hit(2), issue_hit(3)]));
        replies.push(issues(&[1, 2, 3], LOTS));
        replies.extend((0..3).map(|_| no_cross_references(LOTS)));
        let harness = Harness::new("filter-shared", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a", "b"]));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(found_by(&harness, "a"), [1, 2]);
        assert_eq!(found_by(&harness, "b"), [2, 3]);
        harness.finish();
    }

    #[test]
    fn two_filters_with_one_search_run_it_once() {
        let mut replies = searches_finding(&[]);
        replies.push(all_of(&[]));
        let harness = Harness::new("filter-twice", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a", "a"]));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.operations().len(), 5);
        harness.finish();
    }

    #[test]
    fn saving_what_a_filter_found_tells_the_window() {
        let mut replies = searches_finding(&[]);
        replies.push(all_of(&[]));
        let harness = Harness::new("filter-announced", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));

        let changed = run_to_the_end(&mut syncs, &harness);

        assert_eq!(changed, [cpython()]);
        harness.finish();
    }

    #[test]
    fn a_second_sync_fetches_only_what_changed_and_still_reads_each_tree() {
        const SAVED: &str = "2026-11-04T10:00:00Z";
        const LATER: &str = "2026-11-05T10:00:00Z";
        let later_issue = {
            let mut hit = issue_hit(2);
            hit["updatedAt"] = json!(LATER);
            hit
        };
        let mut replies = searches_finding(&[]);
        replies.push(all_of(&[
            issue_hit(1),
            issue_hit(2),
            pr_hit(10, SAVED),
            pr_hit(11, SAVED),
            pr_hit(12, SAVED),
        ]));
        replies.push(fetched_nodes(
            &[
                pr_node(10, &json!({ "updatedAt": SAVED, "title": "gh-3: Fix it" })),
                pr_node(11, &json!({ "updatedAt": SAVED, "title": "PR 11" })),
                pr_node(
                    12,
                    &json!({
                        "updatedAt": SAVED,
                        "title": "PR 12",
                        "state": "MERGED",
                        "mergeCommit": { "oid": "0".repeat(40) },
                    }),
                ),
            ],
            LOTS,
        ));
        replies.push(issues(&[1, 2, 3], LOTS));
        replies.extend((0..3).map(|_| no_cross_references(LOTS)));
        replies.extend(searches_finding(&[]));
        replies.push(all_of(&[
            issue_hit(1),
            later_issue,
            pr_hit(10, SAVED),
            pr_hit(11, LATER),
            pr_hit(12, LATER),
        ]));
        replies.push(fetched(&[11], LOTS));
        replies.push(issues(&[2, 3], LOTS));
        replies.extend((0..3).map(|_| no_cross_references(LOTS)));
        let harness = Harness::new("filter-missing", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));
        drop(run_to_the_end(&mut syncs, &harness));
        drop(harness.operations());

        harness.forget_the_last_sync();
        syncs.add(filtering(&["a"]));
        drop(run_to_the_end(&mut syncs, &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        // Only #11 changed and can change: #10 didn't, and #12 has merged.
        let batch = asked.iter().find(|(name, _)| name == "FetchPullRequests");
        assert_eq!(batch.unwrap().1["ids"], json!(["id11"]));
        // All three trees are read, Issue 1's among them.
        let reads = asked
            .iter()
            .filter(|(name, _)| name == "FetchCrossReferences");
        let mut numbers: Vec<Value> = reads.map(|(_, sent)| sent["number"].clone()).collect();
        numbers.sort_by_key(Value::to_string);
        assert_eq!(numbers, [json!(1), json!(2), json!(3)]);
        harness.finish();
    }

    #[test]
    fn an_issue_a_filter_found_as_tsugiki_has_it_isnt_fetched_again() {
        let mut replies = searches_finding(&[]);
        replies.push(all_of(&[issue_hit(1)]));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.extend(searches_finding(&[]));
        replies.push(all_of(&[issue_hit(1)]));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("filter-held-issue", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));
        drop(run_to_the_end(&mut syncs, &harness));
        drop(harness.operations());

        harness.forget_the_last_sync();
        syncs.add(filtering(&["a"]));
        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            harness.operations()[4..],
            ["SearchFilter", "FetchCrossReferences"]
        );
        harness.finish();
    }

    #[test]
    fn a_closed_pr_that_changed_is_fetched_again_as_it_may_have_reopened() {
        const SAVED: &str = "2026-11-04T10:00:00Z";
        const LATER: &str = "2026-11-05T10:00:00Z";
        let mut replies = searches_finding(&[]);
        replies.push(all_of(&[pr_hit(10, SAVED)]));
        replies.push(fetched_nodes(
            &[pr_node(
                10,
                &json!({ "updatedAt": SAVED, "title": "PR 10", "state": "CLOSED" }),
            )],
            LOTS,
        ));
        replies.extend(searches_finding(&[]));
        replies.push(all_of(&[pr_hit(10, LATER)]));
        replies.push(fetched_nodes(
            &[pr_node(
                10,
                &json!({ "updatedAt": LATER, "title": "PR 10" }),
            )],
            LOTS,
        ));
        let harness = Harness::new("filter-reopened", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));
        drop(run_to_the_end(&mut syncs, &harness));

        harness.forget_the_last_sync();
        syncs.add(filtering(&["a"]));
        drop(run_to_the_end(&mut syncs, &harness));

        let facts = harness.reader.work_facts(&cpython()).unwrap();
        assert_eq!(
            facts.pull_requests[0].state,
            crate::github::pull_requests::PrState::Open
        );
        harness.finish();
    }

    #[test]
    fn a_tree_fetches_again_an_open_pr_a_filter_found_unchanged_as_its_ci_may_have_moved() {
        const SAVED: &str = "2026-11-04T10:00:00Z";
        let mut replies = searches_finding(&[]);
        replies.push(all_of(&[pr_hit(10, SAVED)]));
        replies.push(fetched_nodes(
            &[pr_node(
                10,
                &json!({ "updatedAt": SAVED, "title": "gh-1: Fix it" }),
            )],
            LOTS,
        ));
        replies.push(issues(&[1], LOTS));
        replies.push(cross_references(&[10], None, LOTS));
        replies.extend(searches_finding(&[]));
        replies.push(all_of(&[pr_hit(10, SAVED)]));
        replies.push(issues(&[1], LOTS));
        replies.push(cross_references(&[10], None, LOTS));
        replies.push(fetched_nodes(
            &[pr_node(
                10,
                &json!({ "updatedAt": SAVED, "title": "gh-1: Fix it" }),
            )],
            LOTS,
        ));
        let harness = Harness::new("filter-tree", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));
        drop(run_to_the_end(&mut syncs, &harness));
        drop(harness.operations());

        harness.forget_the_last_sync();
        syncs.add(filtering(&["a"]));
        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            harness.operations()[4..],
            [
                "SearchFilter",
                "FetchIssues",
                "FetchCrossReferences",
                "FetchTreePullRequests"
            ]
        );
        harness.finish();
    }

    #[test]
    fn a_pr_the_users_searches_and_a_filter_both_found_is_fetched_once() {
        let mut replies = searches_finding(&["id10"]);
        replies.push(all_of(&[pr_hit(10, "2026-11-04T10:00:00Z")]));
        replies.push(fetched(&[10], LOTS));
        let harness = Harness::new("filter-and-work", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[5].1["ids"], json!(["id10"]));
        harness.finish();
    }

    #[test]
    fn a_search_the_user_edited_away_loses_its_results_at_the_next_sync() {
        let mut replies = searches_finding(&[]);
        replies.push(all_of(&[issue_hit(1)]));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.extend(searches_finding(&[]));
        replies.push(all_of(&[]));
        let harness = Harness::new("filter-edited", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["old"]));
        drop(run_to_the_end(&mut syncs, &harness));

        harness.forget_the_last_sync();
        syncs.add(filtering(&["new"]));
        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(found_by(&harness, "old"), [0_u32; 0]);
        harness.finish();
    }

    #[test]
    fn a_filter_github_wont_answer_says_why_and_stops_nothing_else() {
        let mut replies = searches_finding(&[]);
        replies.push(all_of(&[issue_hit(1)]));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.extend(searches_finding(&[]));
        replies.push(reply_ok(
            r#"{"errors":[{"message":"That search is too long"}]}"#,
        ));
        replies.push(all_of(&[issue_hit(2)]));
        replies.push(issues(&[2], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("filter-refused", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));
        drop(run_to_the_end(&mut syncs, &harness));

        harness.forget_the_last_sync();
        syncs.add(filtering(&["a", "b"]));
        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            filter_problem(&harness, "a").as_deref(),
            Some("GitHub said: That search is too long")
        );
        assert_eq!(found_by(&harness, "a"), [1]);
        assert_eq!(found_by(&harness, "b"), [2]);
        harness.finish();
    }

    #[test]
    fn a_filter_github_wont_answer_tells_the_window() {
        let mut replies = searches_finding(&[]);
        replies.push(reply_ok(
            r#"{"errors":[{"message":"That search is too long"}]}"#,
        ));
        let harness = Harness::new("filter-refused-announced", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));

        let changed = run_to_the_end(&mut syncs, &harness);

        assert_eq!(changed, [cpython()]);
        harness.finish();
    }

    #[test]
    fn an_outage_blames_no_filter_and_its_search_runs_again() {
        let mut replies = searches_finding(&[]);
        replies.push(Ok(Reply {
            status: 503,
            reset: None,
            body: String::new(),
        }));
        replies.push(all_of(&[issue_hit(1)]));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("filter-outage", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));

        // Four searches and a move.
        for _ in 0..5 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }
        let failed = syncs.step(&harness.github, &harness.store).unwrap_err();
        assert_eq!(filter_problem(&harness, "a"), None);
        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(failed.kind, ErrorKind::Internal);
        assert_eq!(filter_problem(&harness, "a"), None);
        assert_eq!(found_by(&harness, "a"), [1]);
        harness.finish();
    }

    #[test]
    fn with_no_network_a_filters_search_waits_and_runs_again() {
        let mut replies = searches_finding(&[]);
        replies.push(Err(Unreachable));
        replies.push(Err(Unreachable));
        replies.push(all_of(&[]));
        let harness = Harness::new("filter-offline", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));

        // Four searches and a move.
        for _ in 0..5 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }
        let refused = syncs.step(&harness.github, &harness.store).unwrap_err();
        assert_eq!(filter_problem(&harness, "a"), None);
        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(refused.kind, ErrorKind::Offline);
        assert_eq!(filter_problem(&harness, "a"), None);
        let asked = harness.operations();
        assert_eq!(asked[4..], ["SearchFilter"; 3]);
        harness.finish();
    }

    #[test]
    fn a_filter_search_below_the_floor_holds_the_next_step_back() {
        let mut replies = searches_finding(&[]);
        let page = |next: Option<&str>| {
            let info = json!({ "hasNextPage": next.is_some(), "endCursor": next });
            reply_ok(&format!(
                r#"{{"data":{{"search":{{"issueCount":0,"pageInfo":{info},"nodes":[]}},{FEW}}}}}"#
            ))
        };
        replies.push(page(Some("more")));
        replies.push(page(None));
        let harness = Harness::new("filter-floor", replies);
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));
        let now = Instant::now();

        // Four searches and a move, then the first page.
        for _ in 0..6 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }
        assert!(syncs.ready_at().unwrap() > now + Duration::from_secs(60));
        drop(syncs.step(&harness.github, &harness.store).unwrap());
        assert!(syncs.ready_at().unwrap() > now + Duration::from_secs(60));
        harness.finish();
    }

    /// Answers as GitHub would for a Repository with these items, listing at most 1,000 for one search.
    /// `crowded_day` holds 1,500 more items that the search counts and never lists.
    struct ByCreated {
        items: Vec<(u32, &'static str)>,
        crowded_day: Option<&'static str>,
        asked: std::sync::mpsc::Sender<Value>,
    }

    impl Transport for ByCreated {
        fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
            match request.operation {
                "SearchFilter" => {
                    drop(self.asked.send(request.variables.clone()));
                    Ok(self.answer(request.variables["query"].as_str().unwrap_or_default()))
                }
                "FetchIssues" => issues(&[], LOTS),
                "FetchCrossReferences" => no_cross_references(LOTS),
                _ => nothing(LOTS),
            }
        }
    }

    impl ByCreated {
        fn answer(&self, query: &str) -> Reply {
            let range = query.split_once("created:").map(|(_, range)| {
                let (from, to) = range.split_once("..").unwrap();
                (from.to_owned(), to.to_owned())
            });
            let within = |day: &str| {
                range
                    .as_ref()
                    .is_none_or(|(from, to)| from.as_str() <= day && day <= to.as_str())
            };
            let nodes: Vec<Value> = self
                .items
                .iter()
                .filter(|(_, day)| within(day))
                .map(|(number, _)| issue_hit(*number))
                .collect();
            let crowd = self
                .crowded_day
                .filter(|day| within(day))
                .map_or(0, |_| 1500);
            let total = u32::try_from(nodes.len()).unwrap() + crowd;
            filter_page(total.max(u32::from(range.is_none()) * 1001), &nodes, None).unwrap()
        }
    }

    fn walking(name: &str, github: ByCreated) -> Harness {
        Harness::answering(name, Box::new(github))
    }

    #[test]
    fn a_filter_over_the_cap_is_read_through_ranges_of_days_and_finds_everything() {
        let (asked, seen) = std::sync::mpsc::channel();
        let harness = walking(
            "filter-cap",
            ByCreated {
                items: vec![(1, "2001-02-03"), (2, "2019-06-07"), (3, "2026-01-02")],
                crowded_day: None,
                asked,
            },
        );
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(found_by(&harness, "a"), [1, 2, 3]);
        assert_eq!(filter_problem(&harness, "a"), None);
        let first: Vec<Value> = seen.try_iter().collect();
        assert_eq!(first[0]["query"], "repo:python/cpython (a)");
        // Halving stops as soon as a range fits: no range below the whole is over the cap.
        assert_eq!(first.len(), 3);
        harness.finish();
    }

    #[test]
    fn a_narrowed_search_github_cuts_off_removes_nothing_and_counts_as_a_change() {
        let (asked, _seen) = std::sync::mpsc::channel();
        let harness = walking(
            "filter-narrow-crowded",
            ByCreated {
                items: vec![(9, "2019-06-07")],
                crowded_day: Some("2019-06-07"),
                asked,
            },
        );
        let held = FilterResults {
            numbers: BTreeSet::from([8, 9]),
            problem: None,
        };
        let search = filter_query("a");
        harness
            .store
            .save_filter_results(&cpython(), &search, held)
            .unwrap();
        let mut walk = FilterWalk::of_refresh(RefreshWalk {
            narrowed: vec![search],
            since: Some(OffsetDateTime::UNIX_EPOCH),
            changed: BTreeSet::from([8]),
            ..RefreshWalk::default()
        });
        let repository = cpython();
        let link = Link {
            github: &harness.github,
            store: &harness.store,
            repository: &repository,
            filters: &[],
        };

        let mut said_nothing_new = false;
        loop {
            let (step, next) = walk_filters(&link, &mut walk).unwrap();
            if let Step::Saved { touched, .. } = step {
                said_nothing_new = touched.is_empty();
            }
            if next.is_some() {
                break;
            }
        }

        assert!(said_nothing_new);
        assert_eq!(found_by(&harness, "a"), [8, 9]);
        assert_eq!(
            filter_problem(&harness, "a").as_deref(),
            Some(FilterProblem::CutOff.words())
        );
        harness.finish();
    }

    #[test]
    fn one_day_over_the_cap_is_read_as_far_as_github_lists_and_the_filter_says_so() {
        let (asked, _seen) = std::sync::mpsc::channel();
        let harness = walking(
            "filter-crowded",
            ByCreated {
                items: vec![(1, "2019-06-07"), (2, "2019-06-08")],
                crowded_day: Some("2019-06-07"),
                asked,
            },
        );
        let mut syncs = Syncs::default();
        syncs.add(filtering(&["a"]));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(found_by(&harness, "a"), [1, 2]);
        assert_eq!(
            filter_problem(&harness, "a").as_deref(),
            Some(FilterProblem::CutOff.words())
        );
        harness.finish();
    }
}
