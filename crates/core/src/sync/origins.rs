//! Fetching the Origin PR a Backport names, when Tsugiki lacks it (ADR 0011).

use std::collections::BTreeSet;

use super::{Leads, Link, Moved, Resumable, Step, pause_until};
use crate::error::Error;
use crate::github::pull_requests::PrFetch;
use crate::links::possible_numbers;

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
            PrFetch::Tree | PrFetch::Changed => None,
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
mod tests {
    use crate::github::testing::reply_ok;
    use crate::github::{Reply, Unreachable};
    use crate::sync::Syncs;
    use crate::sync::recovery::{Scenario, recovers_from_each_failure, resumes_after_each_step};
    use crate::sync::testing::{
        FEW, Harness, LOTS, comment_by, cpython, cross_references, empty_window_page,
        fetched_nodes, found, hours_ago, issues, merged_node, no_cross_references, nothing, plain,
        pr_node, run_to_the_end, searches_finding, up_to_the_first_issue, window_issue,
        window_page, window_pr,
    };
    use serde_json::{Value, json};
    use std::time::{Duration, Instant};

    /// PR `number`, a Backport of `origin` on 3.14.
    fn backport_node(number: u32, origin: u32) -> Value {
        pr_node(
            number,
            &json!({
                "title": format!("[3.14] gh-1: Fix it (GH-{origin})"),
                "baseRefName": "3.14",
            }),
        )
    }

    fn origin_answer(node: &Value, rate: &str) -> Result<Reply, Unreachable> {
        reply_ok(&format!(
            r#"{{"data":{{"repository":{{"pullRequest":{node}}},{rate}}}}}"#
        ))
    }

    #[test]
    fn a_first_sync_keeps_what_happened_on_the_prs_it_fetched_and_on_their_origin_prs() {
        let timeline = |login: &str| json!({ "nodes": [comment_by(login, &hours_ago(1))] });
        let mut backport = backport_node(1, 9);
        backport["timelineItems"] = timeline("some-reviewer");
        let mut origin = merged_node(9);
        origin["timelineItems"] = timeline("some-author");
        let replies = vec![
            found(&["a"], None, LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            fetched_nodes(&[backport], LOTS),
            origin_answer(&origin, LOTS),
            issues(&[1], LOTS),
            no_cross_references(LOTS),
        ];
        let harness = Harness::new("origin-activity", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let who = |number: u32| -> Vec<String> {
            let events = harness.store.activity(&cpython(), number).unwrap();
            events
                .iter()
                .map(|event| event.who.as_str().to_owned())
                .collect()
        };
        assert_eq!(who(1), ["some-reviewer"]);
        assert_eq!(who(9), ["some-author"]);
        harness.finish();
    }

    #[test]
    fn a_backport_the_window_brought_still_leads_to_its_origin_pr_when_a_tree_lists_it() {
        let mut backport = backport_node(10, 9);
        backport["updatedAt"] = json!(hours_ago(1));
        let mut replies = up_to_the_first_issue();
        replies.push(empty_window_page());
        replies.push(window_page(&[backport], None, LOTS));
        replies.push(cross_references(&[10], None, LOTS));
        replies.push(origin_answer(&merged_node(9), LOTS));
        let harness = Harness::with_window("origin-window", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            harness.operations()[6..],
            [
                "WindowIssues",
                "WindowPullRequests",
                "FetchCrossReferences",
                "FetchOriginPullRequest",
            ]
        );
        assert_eq!(harness.numbers(), [1, 9, 10]);
        harness.finish();
    }

    #[test]
    fn a_backport_only_the_window_brought_doesnt_ask_for_its_origin_pr() {
        let mut backport = backport_node(10, 9);
        backport["updatedAt"] = json!(hours_ago(1));
        let mut replies = up_to_the_first_issue();
        replies.push(empty_window_page());
        replies.push(window_page(&[backport], None, LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::with_window("origin-window-only", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.numbers(), [1, 10]);
        harness.finish();
    }

    #[test]
    fn a_first_sync_recovers_from_a_failure_at_each_point() {
        let posts = vec![
            found(&["a"], Some("next"), LOTS),
            found(&["b"], None, LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            fetched_nodes(&[pr_node(1, &json!({})), backport_node(2, 9)], LOTS),
            origin_answer(&merged_node(9), LOTS),
            issues(&[1], LOTS),
            window_page(&[window_issue(20, &hours_ago(1))], None, LOTS),
            window_page(&[window_pr(21, &hours_ago(2))], Some("older"), LOTS),
            window_page(&[window_pr(22, &hours_ago(3))], None, LOTS),
            cross_references(&[3], Some("older"), LOTS),
            // 21 came with the window, so the tree reads what it knows of it from the store.
            cross_references(&[4, 21], None, LOTS),
            fetched_nodes(&[merged_node(3), backport_node(4, 8)], LOTS),
            origin_answer(&merged_node(8), LOTS),
        ];
        let scenario = Scenario {
            name: "recovery-first",
            settings: plain(cpython()),
            prepare: |_| {},
            posts,
            checks: Vec::new(),
        };

        let operations = recovers_from_each_failure(&scenario);
        resumes_after_each_step(&scenario);

        assert_eq!(
            operations,
            [
                "SearchReviewRequests",
                "SearchReviewRequests",
                "SearchReviewedPrs",
                "SearchOwnPrs",
                "SearchAssignedPrs",
                "FetchPullRequests",
                "FetchOriginPullRequest",
                "FetchIssues",
                "WindowIssues",
                "WindowPullRequests",
                "WindowPullRequests",
                "FetchCrossReferences",
                "FetchCrossReferences",
                "FetchTreePullRequests",
                "FetchOriginPullRequest",
            ]
        );
    }

    #[test]
    fn a_sync_fetches_a_users_backports_missing_origin_pr_by_number() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
        replies.push(origin_answer(&merged_node(9), LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("origin", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        let changed = run_to_the_end(&mut syncs, &harness);

        let (numbers, asked): (Vec<String>, Vec<Value>) = harness.asked.try_iter().unzip();
        assert_eq!(
            numbers[4..],
            [
                "FetchPullRequests",
                "FetchOriginPullRequest",
                "FetchIssues",
                "FetchCrossReferences",
            ]
        );
        assert_eq!(asked[5]["number"], 9);
        assert_eq!(harness.numbers(), [2, 9]);
        assert_eq!(changed.len(), 3);
        harness.finish();
    }

    #[test]
    fn a_sync_fetches_the_issues_a_users_backports_origin_pr_names() {
        let mut replies = searches_finding(&["a"]);
        let untitled = pr_node(
            2,
            &json!({ "title": "[3.14] Fix it (GH-9)", "baseRefName": "3.14" }),
        );
        replies.push(fetched_nodes(&[untitled], LOTS));
        replies.push(origin_answer(&merged_node(9), LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("origin-issue", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let facts = harness.reader.work_facts(&cpython()).unwrap();
        let stored: Vec<u32> = facts.issues.iter().map(|issue| issue.number).collect();
        assert_eq!(stored, [1]);
        harness.finish();
    }

    #[test]
    fn a_sync_fetches_a_tree_backports_missing_origin_pr_after_the_tree() {
        let mut replies = up_to_the_first_issue();
        replies.push(cross_references(&[2], None, LOTS));
        replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
        replies.push(origin_answer(&merged_node(9), LOTS));
        let harness = Harness::new("tree-origin", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.numbers(), [1, 2, 9]);
        harness.finish();
    }

    #[test]
    fn a_sync_doesnt_ask_for_an_origin_pr_it_has_fetched_or_frozen() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(
            &[pr_node(9, &json!({})), backport_node(2, 9)],
            LOTS,
        ));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        // The next sync finds only the Backport, and its Origin PR has merged by then.
        replies.extend(searches_finding(&["a"]));
        replies.push(fetched_nodes(&[backport_node(2, 8)], LOTS));
        replies.push(origin_answer(&merged_node(8), LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.extend(searches_finding(&["a"]));
        replies.push(fetched_nodes(&[backport_node(2, 8)], LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("origin-known", replies);
        let mut syncs = Syncs::default();

        let origins_in_each_sync: Vec<usize> = (0..3)
            .map(|_| {
                harness.forget_the_last_sync();
                syncs.add(plain(cpython()));
                drop(run_to_the_end(&mut syncs, &harness));
                harness
                    .operations()
                    .iter()
                    .filter(|operation| *operation == "FetchOriginPullRequest")
                    .count()
            })
            .collect();

        assert_eq!(origins_in_each_sync, [0, 1, 0]);
        harness.finish();
    }

    #[test]
    fn a_frozen_origin_pr_still_leads_a_new_backport_to_its_issue() {
        let untitled = |number: u32| {
            pr_node(
                number,
                &json!({ "title": "[3.14] Fix it (GH-9)", "baseRefName": "3.14" }),
            )
        };
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(&[untitled(2)], LOTS));
        replies.push(origin_answer(&merged_node(9), LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.extend(searches_finding(&["b"]));
        replies.push(fetched_nodes(&[untitled(3)], LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(cross_references(&[], None, LOTS));
        let harness = Harness::new("origin-frozen", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        drop(run_to_the_end(&mut syncs, &harness));
        drop(harness.operations());

        harness.forget_the_last_sync();
        syncs.add(plain(cpython()));
        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            harness.operations()[4..],
            ["FetchPullRequests", "FetchIssues", "FetchCrossReferences"]
        );
        harness.finish();
    }

    #[test]
    fn an_origin_number_that_names_no_pr_is_asked_for_once() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
        replies.push(reply_ok(&format!(
        r#"{{"data":{{"repository":{{"pullRequest":null}},{LOTS}}},"errors":[{{"type":"NOT_FOUND",
                "message":"gone","path":["repository","pullRequest"]}}]}}"#
    )));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("origin-gone", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.numbers(), [2]);
        harness.finish();
    }

    #[test]
    fn an_origin_fetch_below_the_floor_holds_the_next_step_back() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
        replies.push(origin_answer(&merged_node(9), FEW));
        let harness = Harness::new("origin-floor", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = Instant::now();

        // Four searches, two moves, the PR, a move and the Origin PR.
        for _ in 0..9 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }

        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
        harness.finish();
    }
}
