//! Each rule of Needs attention as a scenario in the glossary's words, read the way the window reads it.

mod archive;
mod mute;

use std::collections::BTreeMap;

use time::{Duration, OffsetDateTime};

use super::Attention;
use crate::facts::CommitSha;
use crate::facts::activity::{Activity, ActivityKind, ItemKey};
use crate::facts::filters::FilterResults;
use crate::facts::issue_content::IssueContent;
use crate::facts::issues::IssueFacts;
use crate::facts::patch_ids::{
    CantCheck, CommitPatches, FilePatch, PatchId, PatchesOf, UncheckedCommit,
};
use crate::facts::pull_request_content::{PullRequestContent, Review};
use crate::facts::pull_requests::{CiState, LatestReview, PrState, PullRequestFacts, ReviewState};
use crate::filters::FilterQuery;
use crate::login::Login;
use crate::patch_ids::ChangedFile;
use crate::settings::ListOrder;
use crate::store::Flagging;
use crate::store::testing::World;
use crate::testing::{cpython, cpython_settings, hours_ago, issue_facts, me, pr_facts, sha_of};
use crate::work::{SeenMark, Why, YourReview};
use crate::workbench::{
    ListedItem, Seen, SeenState, TreeEntry, Workbench, WorkbenchItem, WorkbenchView,
};

fn colleague() -> Login {
    Login::new("some-author")
}

fn reviewer() -> Login {
    Login::new("some-reviewer")
}

/// A colleague's open PR that fixes no Issue, and has nothing to do with the user.
fn pull_request(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        title: format!("Fix it {number}"),
        author: Some(colleague()),
        updated_at: hours_ago(100),
        ..pr_facts(number)
    }
}

fn own(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        author: Some(me()),
        ..pull_request(number)
    }
}

fn reviewed_by_me(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        latest_reviews: vec![LatestReview {
            author: me(),
            state: ReviewState::Commented,
        }],
        ..pull_request(number)
    }
}

/// Own PR `number`, its CI still running.
fn own_ci_pending(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        ci: Some(CiState::Pending),
        ..own(number)
    }
}

fn requested(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        review_requests: vec![me()],
        ..pull_request(number)
    }
}

/// A PR that fixes Issue `issue`, which the title says.
fn fixing(number: u32, issue: u32) -> PullRequestFacts {
    fixing_too(pull_request(number), issue)
}

/// `pull_request`, with a title that says it fixes Issue `issue`.
fn fixing_too(pull_request: PullRequestFacts, issue: u32) -> PullRequestFacts {
    PullRequestFacts {
        title: format!("gh-{issue}: Fix it"),
        ..pull_request
    }
}

fn issue(number: u32) -> IssueFacts {
    IssueFacts {
        author: Some(Login::new("issue-author")),
        updated_at: hours_ago(100),
        ..issue_facts(number)
    }
}

fn comment(who: &Login, at: OffsetDateTime) -> Activity {
    Activity {
        kind: ActivityKind::Comment,
        who: who.clone(),
        at,
    }
}

/// What the scenarios of Needs attention add to the shared world, and read from it the way the window does.
impl World {
    fn has_issues(&self, issues: Vec<IssueFacts>) {
        self.db.store.save_issues(&cpython(), issues).unwrap();
    }

    /// A sync that finds `pull_request`'s CI failed.
    fn ci_fails(&self, pull_request: PullRequestFacts) {
        self.has(vec![PullRequestFacts {
            ci: Some(CiState::Failure),
            ..pull_request
        }]);
    }

    fn marked_unseen(&self, number: u32, at: OffsetDateTime) {
        let flagging = Flagging::on(number);
        self.db.store.mark_unseen(&cpython(), flagging, at).unwrap();
    }

    /// The mark that Mark Seen on item `number`'s line of the Workbench sends, as the window drew it.
    fn seen(&self, number: u32) -> Seen {
        seen_of(&listed_at(&self.workbench(), number).item)
    }

    /// The key of item `number`'s line of `view`, and the mark Mark Seen on it sends, as the window drew it.
    fn drawn_in(&self, view: &WorkbenchView, number: u32) -> (ItemKey, SeenMark) {
        let listed = self.listed_in(view, number);
        (listed.key, seen_of(&listed.item).mark)
    }

    /// The Filter that searches `query` found `numbers`.
    fn found(&self, query: &FilterQuery, numbers: &[u32]) {
        let results = FilterResults {
            numbers: numbers.iter().copied().collect(),
            problem: None,
        };
        (self.db.store)
            .save_filter_results(&cpython(), query, results)
            .unwrap();
    }

    /// Item `number` as `view` lists it.
    fn listed_in(&self, view: &WorkbenchView, number: u32) -> ListedItem {
        listed_at(&self.view(view), number)
    }

    /// The Sidebar's count beside the Filter that searches `query`.
    fn count_of(&self, query: &FilterQuery) -> u32 {
        let lines = self.workbench().filters;
        let line = lines.iter().find(|line| line.query == *query).unwrap();
        line.count
    }

    /// Mark Seen with what the window drew in `seen`.
    fn mark_seen(&self, seen: Seen) {
        let SeenMark { numbers, up_to } = seen.mark;
        self.db.store.mark_seen(&cpython(), numbers, up_to).unwrap();
    }

    fn view(&self, view: &WorkbenchView) -> Workbench {
        let settings = cpython_settings();
        Workbench::read(&self.db.reader, &settings, view, ListOrder::Attention).unwrap()
    }

    fn workbench(&self) -> Workbench {
        self.view(&WorkbenchView::Whole)
    }

    /// The reasons item `number`, wherever the Workbench lists it, Needs attention. None, if it doesn't.
    fn why(&self, number: u32) -> Vec<Why> {
        let workbench = self.workbench();
        let listed = listed_at(&workbench, number);
        listed
            .attention
            .map_or_else(Vec::new, |attention| attention.why)
    }

    /// True when `view` lists item `number`.
    fn lists(&self, view: &WorkbenchView, number: u32) -> bool {
        let items = self.view(view).items;
        items.iter().any(|listed| number_of(listed) == number)
    }

    fn on_workbench(&self, number: u32) -> bool {
        self.lists(&WorkbenchView::Whole, number)
    }

    /// The reasons item `number` Needs attention, as `view` lists it.
    fn why_in(&self, view: &WorkbenchView, number: u32) -> Vec<Why> {
        let listed = listed_at(&self.view(view), number);
        listed
            .attention
            .map_or_else(Vec::new, |attention| attention.why)
    }

    fn since(&self, number: u32) -> OffsetDateTime {
        let workbench = self.workbench();
        let attention: Attention = listed_at(&workbench, number).attention.unwrap();
        attention.since
    }

    /// Own PR 1 with a colleague's comment, which the user Marked unseen since. Gives when the comment came.
    fn has_a_flagged_comment(&self) -> OffsetDateTime {
        self.has(vec![own(1)]);
        let commented = hours_ago(2);
        self.happened(1, vec![comment(&colleague(), commented)]);
        self.marked_unseen(1, hours_ago(1));
        commented
    }

    /// Fails unless the user saw PR 1 up to `at`, and nothing is Marked unseen.
    fn assert_seen_up_to(&self, at: OffsetDateTime) {
        let facts = self.db.reader.work_facts(&cpython()).unwrap().activity;
        assert_eq!(facts.marked_unseen, BTreeMap::new());
        assert_eq!(facts.last_seen.get(&1), Some(&at));
    }

    /// PR 1, which asks the user for a review from five hours ago, and the "Involved" Filter finds. Gives the
    /// PR, for `request_ends`.
    fn has_a_requested_pr_involved_finds(&self) -> PullRequestFacts {
        let pull_request = requested(1);
        self.has(vec![pull_request.clone()]);
        self.happened(1, vec![review_requested(hours_ago(5))]);
        self.found(&involved(), &[1]);
        pull_request
    }

    /// The Review request on `pull_request` ends, and fails unless Review requests and "Involved" drop it.
    fn request_ends(&self, pull_request: PullRequestFacts) {
        self.has(vec![PullRequestFacts {
            review_requests: vec![],
            updated_at: hours_ago(4),
            ..pull_request
        }]);
        assert!(!self.lists(&WorkbenchView::ReviewRequests, 1));
        assert!(!self.lists(&filter(involved()), 1));
    }
}

/// The part in Mark Seen of `item`'s own line.
fn seen_of(item: &WorkbenchItem) -> Seen {
    match item {
        WorkbenchItem::Issue(issue) => issue.seen.clone(),
        WorkbenchItem::StandalonePr(tree) => tree.row.seen.clone(),
        WorkbenchItem::StandaloneBackport(backport) => backport.row.seen.clone(),
    }
}

/// The starter Filters: "Mine", on the Workbench, and "Involved", off it.
fn mine() -> FilterQuery {
    cpython_settings().filters[0].query.clone()
}

fn involved() -> FilterQuery {
    cpython_settings().filters[1].query.clone()
}

fn filter(query: FilterQuery) -> WorkbenchView {
    WorkbenchView::Filter { query }
}

fn number_of(listed: &ListedItem) -> u32 {
    listed.item.updated_at_and_number().1
}

fn listed_at(workbench: &Workbench, number: u32) -> ListedItem {
    let mut found = workbench
        .items
        .iter()
        .filter(|listed| number_of(listed) == number);
    found.next().unwrap().clone()
}

fn numbers(list: &[ListedItem]) -> Vec<u32> {
    list.iter().map(number_of).collect()
}

#[test]
fn a_review_request_needs_attention_from_when_it_was_asked() {
    let world = World::new("attention-review-request");
    world.has(vec![requested(1)]);
    let asked = hours_ago(5);
    world.happened(
        1,
        vec![Activity {
            kind: ActivityKind::ReviewRequested,
            who: me(),
            at: asked,
        }],
    );

    assert_eq!(world.why(1), [Why::ReviewRequested]);
    assert_eq!(world.since(1), asked);
    world.finish();
}

#[test]
fn a_review_request_on_a_colleagues_draft_needs_attention() {
    let world = World::new("attention-draft-request");
    world.has(vec![PullRequestFacts {
        is_draft: true,
        ..requested(1)
    }]);

    assert_eq!(world.why(1), [Why::ReviewRequested]);
    world.finish();
}

fn review_requested(at: OffsetDateTime) -> Activity {
    Activity {
        kind: ActivityKind::ReviewRequested,
        who: me(),
        at,
    }
}

#[test]
fn mark_seen_takes_a_review_request_out_of_needs_attention_and_a_new_request_brings_it_back() {
    let world = World::new("attention-request-seen");
    world.has(vec![requested(1)]);
    world.happened(1, vec![review_requested(hours_ago(5))]);

    world.mark_seen(world.seen(1));
    // It was on the Workbench only because it Needed attention.
    assert!(!world.on_workbench(1));
    let requests = WorkbenchView::ReviewRequests;
    assert_eq!(world.why_in(&requests, 1), []);
    assert_eq!(world.view(&requests).review_requests, 1);

    let asked_again = hours_ago(1);
    world.happened(1, vec![review_requested(asked_again)]);
    assert_eq!(world.why(1), [Why::ReviewRequested]);
    assert_eq!(world.since(1), asked_again);
    world.finish();
}

#[test]
fn a_write_that_moves_only_last_seen_leaves_a_review_request_waiting() {
    let world = World::new("attention-request-write");
    world.has(vec![requested(1)]);
    world.happened(1, vec![review_requested(hours_ago(5))]);
    let mut facts = world.db.reader.work_facts(&cpython()).unwrap();
    // A write to GitHub moves Last seen alone (#395).
    let _moved = facts.activity.last_seen.insert(1, hours_ago(1));

    let workbench = Workbench::from_facts(facts, &cpython_settings(), &WorkbenchView::Whole);

    let attention = listed_at(&workbench, 1).attention.unwrap();
    assert_eq!(attention.why, [Why::ReviewRequested]);
    world.finish();
}

#[test]
fn a_review_request_with_no_event_left_on_the_timeline_counts_from_the_last_update() {
    let world = World::new("attention-request-no-event");
    let updated_at = hours_ago(9);
    world.has(vec![PullRequestFacts {
        updated_at,
        ..requested(1)
    }]);

    assert_eq!(world.since(1), updated_at);
    world.finish();
}

#[test]
fn mark_seen_acknowledges_a_review_request_with_no_event_whatever_updates_the_pr_later() {
    let world = World::new("attention-request-no-event-seen");
    let pull_request = PullRequestFacts {
        updated_at: hours_ago(9),
        ..requested(1)
    };
    world.has(vec![pull_request.clone()]);
    world.mark_seen(world.seen(1));

    // A colleague's label or comment moves the last update, and asks nothing new of the user.
    world.has(vec![PullRequestFacts {
        updated_at: hours_ago(1),
        ..pull_request
    }]);

    assert_eq!(world.why_in(&WorkbenchView::ReviewRequests, 1), []);
    world.finish();
}

#[test]
fn someone_elses_review_request_is_no_time_for_the_users() {
    let world = World::new("attention-request-other");
    let updated_at = hours_ago(9);
    world.has(vec![PullRequestFacts {
        updated_at,
        ..requested(1)
    }]);
    world.happened(
        1,
        vec![Activity {
            kind: ActivityKind::ReviewRequested,
            who: reviewer(),
            at: hours_ago(2),
        }],
    );

    assert_eq!(world.since(1), updated_at);
    world.finish();
}

#[test]
fn an_assignee_has_a_review_request_until_they_submit_a_review() {
    let world = World::new("attention-assignee");
    let assigned = PullRequestFacts {
        assignees: vec![me()],
        ..pull_request(1)
    };
    world.has(vec![assigned.clone()]);
    let at = hours_ago(6);
    let event = Activity {
        kind: ActivityKind::Assigned,
        who: me(),
        at,
    };
    world.happened(1, vec![event]);
    assert_eq!(world.why(1), [Why::ReviewRequested]);
    assert_eq!(world.since(1), at);

    world.has(vec![PullRequestFacts {
        latest_reviews: vec![LatestReview {
            author: me(),
            state: ReviewState::Approved,
        }],
        ..assigned
    }]);

    assert_eq!(world.why(1), []);
    world.finish();
}

#[test]
fn assigning_yourself_to_your_own_pr_asks_nothing_of_you() {
    let world = World::new("attention-self-assigned");
    world.has(vec![PullRequestFacts {
        assignees: vec![me()],
        ..own(1)
    }]);

    assert_eq!(world.why(1), []);
    assert_eq!(world.workbench().review_requests, 0);
    world.finish();
}

#[test]
fn mark_seen_takes_failed_ci_on_an_own_pr_out_of_needs_attention_and_keeps_the_row() {
    let world = World::new("attention-own-ci");
    world.has(vec![PullRequestFacts {
        ci: Some(CiState::Failure),
        ..own(1)
    }]);
    assert_eq!(world.why(1), [Why::CiFailed { checks: vec![] }]);

    world.mark_seen(world.seen(1));

    assert_eq!(world.why(1), []);
    world.finish();
}

#[test]
fn ci_that_fails_after_mark_seen_needs_attention() {
    let world = World::new("attention-own-ci-after");
    let pending = own_ci_pending(1);
    world.has(vec![pending.clone()]);
    world.mark_seen(world.seen(1));

    world.ci_fails(pending);

    assert_eq!(world.why(1), [Why::CiFailed { checks: vec![] }]);
    world.finish();
}

#[test]
fn ci_that_fails_again_after_a_push_needs_attention_though_no_pending_state_came_between() {
    let world = World::new("attention-own-ci-again");
    world.has(vec![PullRequestFacts {
        ci: Some(CiState::Failure),
        ..own(1)
    }]);
    let mut facts = world.db.reader.work_facts(&cpython()).unwrap();
    // The store keeps whole seconds, so the moments are set apart by hand.
    let activity = &mut facts.activity;
    let _failed = activity.ci_finished.insert(1, hours_ago(5));
    let _acknowledged = activity.acknowledged.insert(1, hours_ago(4));
    let push = Activity {
        kind: ActivityKind::Commit,
        who: me(),
        at: hours_ago(1),
    };
    let _pushed = activity.events.insert(1, vec![push]);

    let workbench = Workbench::from_facts(facts, &cpython_settings(), &WorkbenchView::Whole);

    let attention = listed_at(&workbench, 1).attention.unwrap();
    assert_eq!(attention.why, [Why::CiFailed { checks: vec![] }]);
    world.finish();
}

#[test]
fn failed_ci_names_the_checks_once_tsugiki_has_asked() {
    let world = World::new("attention-own-ci-names");
    world.has(vec![PullRequestFacts {
        ci: Some(CiState::Failure),
        ..own(1)
    }]);
    let names = vec!["Tests / Ubuntu".to_owned()];
    world
        .db
        .store
        .save_failed_checks(&cpython(), vec![(1, names.clone())])
        .unwrap();

    assert_eq!(world.why(1), [Why::CiFailed { checks: names }]);
    world.finish();
}

#[test]
fn failed_ci_on_a_merged_own_pr_waits_on_nobody() {
    let world = World::new("attention-merged-ci");
    world.has(vec![
        own(2),
        PullRequestFacts {
            state: PrState::Merged,
            ci: Some(CiState::Failure),
            updated_at: hours_ago(100),
            ..own(1)
        },
    ]);

    let workbench = world.workbench();

    assert_eq!(numbers(&workbench.items), [2]);
    world.finish();
}

#[test]
fn failed_ci_on_a_colleagues_pr_doesnt_list_it() {
    let world = World::new("attention-colleague-ci");
    world.has(vec![PullRequestFacts {
        ci: Some(CiState::Failure),
        ..pull_request(1)
    }]);

    assert_eq!(world.workbench().items, []);
    world.finish();
}

#[test]
fn a_passing_run_on_an_own_pr_is_new_activity_until_it_is_marked_seen() {
    let world = World::new("attention-own-ci-pass");
    world.has(vec![PullRequestFacts {
        ci: Some(CiState::Success),
        ..own(1)
    }]);
    assert_eq!(world.why(1), [Why::CiPassed]);

    world.marked_seen(1, OffsetDateTime::now_utc() + Duration::hours(1));

    assert_eq!(world.why(1), []);
    world.finish();
}

#[test]
fn new_activity_on_a_pr_the_user_wrote_or_reviewed_needs_attention() {
    let world = World::new("attention-wrote-or-reviewed");
    world.has(vec![own(1), reviewed_by_me(2)]);
    let by_colleague = |kind| Activity {
        kind,
        who: colleague(),
        at: hours_ago(2),
    };
    world.happened(1, vec![by_colleague(ActivityKind::Commit)]);
    world.happened(2, vec![by_colleague(ActivityKind::ReadyForReview)]);

    assert_eq!(world.why(1), [Why::Commits { by: colleague() }]);
    assert_eq!(world.why(2), [Why::ReadyForReview { by: colleague() }]);
    world.finish();
}

#[test]
fn a_review_on_an_own_pr_says_what_the_reviewer_decided() {
    let world = World::new("attention-review-words");
    let review = |author: Login, state| LatestReview { author, state };
    let approver = Login::new("another-account");
    let commenter = Login::new("some-assignee");
    world.has(vec![PullRequestFacts {
        latest_reviews: vec![
            review(reviewer(), ReviewState::ChangesRequested),
            review(approver.clone(), ReviewState::Approved),
            review(commenter.clone(), ReviewState::Commented),
        ],
        ..own(1)
    }]);
    let submitted = |who: &Login, hours| Activity {
        kind: ActivityKind::Review,
        who: who.clone(),
        at: hours_ago(hours),
    };
    world.happened(
        1,
        vec![
            submitted(&reviewer(), 1),
            submitted(&approver, 2),
            submitted(&commenter, 3),
        ],
    );

    assert_eq!(
        world.why(1),
        [
            Why::ChangesRequested { by: reviewer() },
            Why::Approved { by: approver },
            Why::Reviewed { by: commenter },
        ]
    );
    world.finish();
}

#[test]
fn a_failed_bot_backport_needs_attention_on_a_merged_origin_pr() {
    let world = World::new("attention-backport-conflict");
    world.has(vec![PullRequestFacts {
        state: PrState::Merged,
        ..own(1)
    }]);
    world.happened(
        1,
        vec![Activity {
            kind: ActivityKind::BackportFailed {
                target: "3.14".to_owned(),
            },
            who: Login::new("miss-islington-app"),
            at: hours_ago(1),
        }],
    );

    assert_eq!(
        world.why(1),
        [Why::BackportFailed {
            target: "3.14".to_owned()
        }]
    );
    world.finish();
}

#[test]
fn an_unreviewed_colleagues_pr_is_not_listed_as_waiting() {
    let world = World::new("attention-unreviewed");
    world.has(vec![pull_request(1)]);
    world.happened(1, vec![comment(&reviewer(), hours_ago(1))]);

    assert_eq!(world.workbench().items, []);
    world.finish();
}

#[test]
fn a_colleagues_draft_with_new_activity_waits_for_a_review_request() {
    let world = World::new("attention-draft");
    world.has(vec![
        PullRequestFacts {
            is_draft: true,
            ..reviewed_by_me(1)
        },
        own(2),
    ]);
    world.happened(1, vec![comment(&reviewer(), hours_ago(1))]);

    assert_eq!(world.why(1), []);
    world.finish();
}

#[test]
fn someone_posting_on_a_pr_the_user_commented_on_needs_attention() {
    let world = World::new("attention-pr-conversation");
    world.has(vec![pull_request(1)]);
    world.happened(
        1,
        vec![
            comment(&me(), hours_ago(100)),
            comment(&colleague(), hours_ago(1)),
        ],
    );

    assert_eq!(world.why(1), [Why::Comment { by: colleague() }]);
    world.finish();
}

#[test]
fn a_mention_of_the_user_on_a_pr_needs_attention() {
    let world = World::new("attention-pr-mention");
    world.has(vec![pull_request(1)]);
    world.happened(
        1,
        vec![Activity {
            kind: ActivityKind::Mentioned,
            who: me(),
            at: hours_ago(1),
        }],
    );

    assert_eq!(world.why(1), [Why::Mentioned]);
    world.finish();
}

#[test]
fn someone_posting_on_an_issue_the_user_opened_is_assigned_or_commented_on_needs_attention() {
    let world = World::new("attention-issue-conversation");
    world.has_issues(vec![
        IssueFacts {
            author: Some(me()),
            ..issue(1)
        },
        IssueFacts {
            assignees: vec![me()],
            ..issue(2)
        },
        issue(3),
    ]);
    let post = comment(&colleague(), hours_ago(1));
    world.happened(1, vec![post.clone()]);
    world.happened(2, vec![post.clone()]);
    world.happened(3, vec![comment(&me(), hours_ago(100)), post]);

    for number in [1, 2, 3] {
        assert_eq!(
            world.why(number),
            [Why::Comment { by: colleague() }],
            "{number}"
        );
    }
    world.finish();
}

#[test]
fn an_issue_the_user_was_unassigned_from_is_no_longer_their_conversation() {
    let world = World::new("attention-issue-unassigned");
    world.has_issues(vec![issue(1), issue(2)]);
    world.found(&involved(), &[1, 2]);
    let assigned = Activity {
        kind: ActivityKind::Assigned,
        who: me(),
        at: hours_ago(5),
    };
    let post = comment(&colleague(), hours_ago(1));
    world.happened(1, vec![assigned, post.clone()]);
    world.happened(2, vec![post]);

    let view = filter(involved());
    assert_eq!(world.why_in(&view, 1), []);
    let state = |number| seen_of(&world.listed_in(&view, number).item).state;
    assert_eq!(
        state(1),
        state(2),
        "the old assignment reads as no involvement"
    );
    world.finish();
}

#[test]
fn a_mention_in_a_new_post_on_an_issue_needs_attention() {
    let world = World::new("attention-issue-mention");
    world.has_issues(vec![issue(1)]);
    world.happened(
        1,
        vec![
            comment(&colleague(), hours_ago(1)),
            Activity {
                kind: ActivityKind::Mentioned,
                who: me(),
                at: hours_ago(1),
            },
        ],
    );

    let why = world.why(1);
    assert!(why.contains(&Why::Mentioned), "{why:?}");
    world.finish();
}

#[test]
fn a_mention_of_someone_else_leaves_an_issue_the_user_only_follows_alone() {
    let world = World::new("attention-issue-only-follows");
    world.has_issues(vec![issue(1)]);
    world.happened(
        1,
        vec![
            comment(&colleague(), hours_ago(1)),
            Activity {
                kind: ActivityKind::Mentioned,
                who: reviewer(),
                at: hours_ago(1),
            },
        ],
    );

    assert_eq!(world.workbench().items, []);
    world.finish();
}

#[test]
fn an_issue_needs_attention_when_a_pr_in_its_tree_does_and_says_why_with_the_newest_first() {
    let world = World::new("attention-tree");
    world.has_issues(vec![issue(5)]);
    world.has(vec![
        PullRequestFacts {
            review_requests: vec![me()],
            ..fixing(1, 5)
        },
        PullRequestFacts {
            author: Some(me()),
            ci: Some(CiState::Failure),
            ..fixing(2, 5)
        },
    ]);
    let asked = hours_ago(8);
    world.happened(
        1,
        vec![Activity {
            kind: ActivityKind::ReviewRequested,
            who: me(),
            at: asked,
        }],
    );

    let attention = listed_at(&world.workbench(), 5).attention.unwrap();

    assert_eq!(attention.since, asked);
    assert_eq!(
        attention.why,
        [Why::CiFailed { checks: vec![] }, Why::ReviewRequested]
    );
    world.finish();
}

#[test]
fn the_same_reason_on_two_prs_of_a_tree_is_said_once() {
    let world = World::new("attention-said-once");
    world.has_issues(vec![issue(5)]);
    let asked = |pr: PullRequestFacts| PullRequestFacts {
        review_requests: vec![me()],
        ..pr
    };
    world.has(vec![asked(fixing(1, 5)), asked(fixing(2, 5))]);

    assert_eq!(world.why(5), [Why::ReviewRequested]);
    world.finish();
}

#[test]
fn what_needs_attention_comes_first_newest_first_then_the_rest_in_one_list() {
    let world = World::new("attention-order");
    world.has_issues(vec![
        IssueFacts {
            updated_at: hours_ago(1),
            ..issue(10)
        },
        IssueFacts {
            updated_at: hours_ago(2),
            ..issue(11)
        },
    ]);
    let order_of = |at| PullRequestFacts {
        updated_at: at,
        ..requested(0)
    };
    world.has(vec![
        PullRequestFacts {
            number: 1,
            ..order_of(hours_ago(30))
        },
        PullRequestFacts {
            number: 2,
            ..order_of(hours_ago(20))
        },
        PullRequestFacts {
            updated_at: hours_ago(3),
            ..own(3)
        },
        PullRequestFacts {
            updated_at: hours_ago(4),
            ..own(4)
        },
        PullRequestFacts {
            updated_at: hours_ago(5),
            author: Some(me()),
            ..fixing(5, 10)
        },
        PullRequestFacts {
            updated_at: hours_ago(5),
            author: Some(me()),
            ..fixing(6, 11)
        },
    ]);

    let workbench = world.workbench();
    let by_update = Workbench::read(
        &world.db.reader,
        &cpython_settings(),
        &WorkbenchView::Whole,
        ListOrder::LastUpdate,
    )
    .unwrap();

    assert_eq!(numbers(&workbench.items), [2, 1, 10, 11, 3, 4]);
    assert_eq!(numbers(&by_update.items), [10, 11, 3, 4, 2, 1]);
    world.finish();
}

#[test]
fn the_sidebar_counts_what_needs_attention_and_review_requests_lists_those_prs() {
    let world = World::new("attention-counts");
    world.has_issues(vec![issue(5)]);
    world.has(vec![
        requested(1),
        own(2),
        PullRequestFacts {
            review_requests: vec![me()],
            ..fixing(3, 5)
        },
        PullRequestFacts {
            author: Some(me()),
            ci: Some(CiState::Failure),
            ..own(4)
        },
    ]);

    let whole = world.workbench();
    assert_eq!(whole.count, 3);
    assert_eq!(whole.review_requests, 2);

    let requests = world.view(&WorkbenchView::ReviewRequests);
    assert_eq!(numbers(&requests.items), [5, 1]);
    assert_eq!(requests.count, 3);
    world.finish();
}

#[test]
fn an_item_marked_unseen_needs_attention_from_the_moment_of_the_flag() {
    let world = World::new("attention-marked-unseen");
    // An Issue the user only follows, a colleague's PR and a colleague's Draft PR need nothing of the user
    // until they flag them.
    world.has_issues(vec![issue(1)]);
    world.has(vec![
        pull_request(2),
        PullRequestFacts {
            is_draft: true,
            ..pull_request(3)
        },
    ]);
    let flagged = hours_ago(1);
    for number in [1, 2, 3] {
        world.marked_unseen(number, flagged);
    }

    for number in [1, 2, 3] {
        assert_eq!(world.why(number), [Why::MarkedUnseen], "{number}");
        assert_eq!(world.since(number), flagged, "{number}");
    }
    world.finish();
}

#[test]
fn a_pr_marked_unseen_brings_its_issue_back_with_it() {
    let world = World::new("attention-marked-unseen-tree");
    world.has_issues(vec![issue(5)]);
    world.has(vec![fixing(1, 5)]);

    world.marked_unseen(1, hours_ago(1));

    assert_eq!(numbers(&world.workbench().items), [5]);
    assert_eq!(world.why(5), [Why::MarkedUnseen]);
    world.finish();
}

#[test]
fn by_attention_sorts_a_fresh_flag_first() {
    let world = World::new("attention-marked-unseen-order");
    world.has_issues(vec![issue(2)]);
    world.has(vec![requested(1)]);
    world.happened(
        1,
        vec![Activity {
            kind: ActivityKind::ReviewRequested,
            who: me(),
            at: hours_ago(5),
        }],
    );

    world.marked_unseen(2, hours_ago(1));

    assert_eq!(numbers(&world.workbench().items), [2, 1]);
    world.finish();
}

#[test]
fn mark_seen_clears_the_flag_and_what_stood_before_it() {
    let world = World::new("attention-mark-seen-clears");
    world.has(vec![
        requested(1),
        PullRequestFacts {
            ci: Some(CiState::Success),
            ..own(2)
        },
    ]);
    world.marked_unseen(1, hours_ago(1));
    world.marked_unseen(2, hours_ago(1));

    world.mark_seen(world.seen(1));
    world.mark_seen(world.seen(2));

    assert_eq!(world.why_in(&WorkbenchView::ReviewRequests, 1), []);
    assert_eq!(world.why(2), []);
    world.finish();
}

#[test]
fn mark_seen_with_nothing_new_clears_the_flag_and_moves_last_seen_to_the_last_update() {
    let world = World::new("attention-mark-seen-flag-only");
    let updated = hours_ago(100);
    world.has(vec![PullRequestFacts {
        updated_at: updated,
        ..own(1)
    }]);
    world.marked_unseen(1, hours_ago(1));
    let seen = world.seen(1);
    assert_eq!(seen.mark.up_to, Some(updated));
    assert_eq!(seen.state, SeenState::MarkedUnseen);

    world.mark_seen(seen);
    // A colleague's commit from before now that a sync brings later is still news.
    world.happened(
        1,
        vec![Activity {
            kind: ActivityKind::Commit,
            who: colleague(),
            at: hours_ago(1),
        }],
    );

    assert_eq!(world.why(1), [Why::Commits { by: colleague() }]);
    assert_eq!(world.seen(1).state, SeenState::NewActivity);
    world.finish();
}

#[test]
fn a_comment_a_sync_brings_after_the_window_drew_the_item_stays_new_after_mark_seen() {
    let world = World::new("attention-mark-seen-drawn");
    world.has(vec![own(1)]);
    let read = hours_ago(3);
    world.happened(1, vec![comment(&colleague(), read)]);
    let drawn = world.seen(1);
    assert_eq!(drawn.mark.up_to, Some(read));
    assert_eq!(drawn.state, SeenState::NewActivity);

    let later = hours_ago(1);
    world.happened(1, vec![comment(&reviewer(), later)]);
    world.mark_seen(drawn);

    assert_eq!(world.why(1), [Why::Comment { by: reviewer() }]);
    assert_eq!(world.since(1), later);
    world.finish();
}

#[test]
fn mark_seen_covers_what_wasnt_news_yet_when_the_window_drew_it() {
    let world = World::new("attention-mark-seen-not-news-yet");
    let passed = |pr: PullRequestFacts| PullRequestFacts {
        ci: Some(CiState::Success),
        ..pr
    };
    world.has(vec![passed(pull_request(1))]);
    world.happened(
        1,
        vec![
            comment(&me(), hours_ago(4)),
            comment(&colleague(), hours_ago(3)),
        ],
    );
    world.mark_seen(world.seen(1));

    // CI passed before the user marked the PR seen, though it was no news to them until they reviewed it.
    world.has(vec![passed(reviewed_by_me(1))]);

    assert_eq!(world.why(1), []);
    world.finish();
}

#[test]
fn mark_seen_on_an_issue_marks_every_pr_in_its_tree_and_on_a_pr_only_that_pr() {
    let world = World::new("attention-mark-seen-tree");
    world.has_issues(vec![issue(5)]);
    world.has(vec![
        PullRequestFacts {
            author: Some(me()),
            ..fixing(1, 5)
        },
        PullRequestFacts {
            author: Some(me()),
            ..fixing(2, 5)
        },
    ]);
    let (older, newer) = (hours_ago(3), hours_ago(2));
    world.happened(1, vec![comment(&colleague(), older)]);
    world.happened(2, vec![comment(&colleague(), newer)]);

    let workbench = world.workbench();
    let WorkbenchItem::Issue(drawn) = &listed_at(&workbench, 5).item else {
        panic!("Issue 5 is an Issue");
    };
    let mut marked = drawn.seen.mark.numbers.clone();
    marked.sort_unstable();
    assert_eq!(marked, [1, 2, 5]);
    assert_eq!(drawn.seen.mark.up_to, Some(newer));
    let rows = drawn.rows();
    let pr_1: &SeenMark = &rows.iter().find(|row| row.number == 1).unwrap().seen.mark;
    assert_eq!(
        pr_1,
        &SeenMark {
            numbers: vec![1],
            up_to: Some(older)
        }
    );

    world
        .db
        .store
        .mark_seen(&cpython(), pr_1.numbers.clone(), pr_1.up_to)
        .unwrap();
    assert_eq!(world.why(5), [Why::Comment { by: colleague() }]);
    assert_eq!(world.since(5), newer);

    world.mark_seen(world.seen(5));
    assert_eq!(world.why(5), []);
    assert_eq!(numbers(&world.workbench().items), [5]);
    world.finish();
}

#[test]
fn mark_seen_on_an_issue_acknowledges_what_stands_across_its_tree() {
    let world = World::new("attention-mark-seen-tree-standing");
    world.has_issues(vec![issue(5)]);
    world.has(vec![
        fixing_too(requested(1), 5),
        PullRequestFacts {
            ci: Some(CiState::Failure),
            ..fixing_too(own(2), 5)
        },
    ]);
    let standing = [Why::CiFailed { checks: vec![] }, Why::ReviewRequested];
    assert_eq!(world.why(5), standing);

    world.mark_seen(world.seen(5));

    assert_eq!(world.why(5), []);
    world.finish();
}

/// `Lib/ast.py` changed in the way `digit` names.
fn ast_changed(digit: char) -> Vec<FilePatch> {
    let id = PatchId::parse(&digit.to_string().repeat(40)).unwrap();
    vec![FilePatch {
        path: "Lib/ast.py".to_owned(),
        verbatim: id.clone(),
        loose: id,
        generated: false,
    }]
}

impl World {
    /// The user reviewed PR `number` at commit 1, which has moved on to commit 2 since, and the check read
    /// the change at commit 1 as `reviewed` and at commit 2 as `head`.
    fn reviewed_and_moved(&self, pull_request: PullRequestFacts, reviewed: char, head: char) {
        self.reviewed_and_moved_unread(pull_request);
        let patches =
            [(sha_of('1'), reviewed), (sha_of('2'), head)].map(|(commit, digit)| CommitPatches {
                of: on_main(commit),
                files: ast_changed(digit),
            });
        self.db
            .store
            .save_patch_ids(&cpython(), patches.to_vec())
            .unwrap();
    }

    /// The user reviewed PR `number` at commit 1, which has moved on to commit 2 since, and the check has
    /// read neither.
    fn reviewed_and_moved_unread(&self, pull_request: PullRequestFacts) {
        let number = pull_request.number;
        self.has(vec![PullRequestFacts {
            head: Some(sha_of('2')),
            ..pull_request
        }]);
        let content = PullRequestContent {
            conversation: IssueContent {
                body_html: String::new(),
                comments: vec![],
                earlier_comments: 0,
                opened_at: None,
            },
            reviews: vec![Review {
                id: None,
                author: Some(me()),
                state: ReviewState::Commented,
                commit: Some(sha_of('1')),
                submitted_at: hours_ago(50),
                body_html: String::new(),
                hidden: None,
                threads: vec![],
            }],
            earlier_reviews: 0,
            head: Some(sha_of('2')),
            pending: vec![],
        };
        self.db
            .store
            .save_pull_request_content(&cpython(), number, content)
            .unwrap();
    }
}

fn on_main(commit: CommitSha) -> PatchesOf {
    PatchesOf {
        commit,
        target_branch: "main".to_owned(),
    }
}

#[test]
fn a_review_whose_check_cant_run_needs_attention_as_a_stale_review() {
    let world = World::new("attention-stale-unchecked");
    world.reviewed_and_moved_unread(reviewed_by_me(1));
    let cloning = UncheckedCommit {
        of: on_main(sha_of('2')),
        reason: CantCheck::Cloning,
    };
    world
        .db
        .store
        .save_unchecked(&cpython(), vec![cloning])
        .unwrap();

    assert_eq!(world.why(1), [Why::StaleReview]);
    world.finish();
}

#[test]
fn a_stale_review_needs_attention_and_counts() {
    let world = World::new("attention-stale");
    let reviewed = reviewed_by_me(1);
    let updated_at = reviewed.updated_at;
    world.reviewed_and_moved(reviewed, 'a', 'b');

    assert_eq!(world.why(1), [Why::StaleReview]);
    assert_eq!(world.since(1), updated_at);
    assert_eq!(world.workbench().count, 1);
    world.finish();
}

#[test]
fn mark_seen_takes_a_stale_review_out_of_needs_attention_until_a_later_push() {
    let world = World::new("attention-stale-seen");
    world.reviewed_and_moved(reviewed_by_me(1), 'a', 'b');

    world.mark_seen(world.seen(1));
    // The user's Review holds it on the Workbench, as a plain row.
    assert_eq!(world.why(1), []);
    let stale = WorkbenchView::StaleReviews;
    assert!(world.lists(&stale, 1));
    assert_eq!(world.view(&stale).stale_reviews, 1);

    let pushed = Activity {
        kind: ActivityKind::Commit,
        who: colleague(),
        at: hours_ago(1),
    };
    world.happened(1, vec![pushed]);
    assert!(world.why(1).contains(&Why::StaleReview));
    world.finish();
}

#[test]
fn a_comment_after_mark_seen_leaves_a_stale_review_acknowledged() {
    let world = World::new("attention-stale-comment");
    let pull_request = reviewed_by_me(1);
    world.reviewed_and_moved(pull_request.clone(), 'a', 'b');
    world.mark_seen(world.seen(1));

    let later = hours_ago(1);
    world.has(vec![PullRequestFacts {
        head: Some(sha_of('2')),
        updated_at: later,
        ..pull_request
    }]);
    world.happened(1, vec![comment(&colleague(), later)]);

    assert_eq!(world.why(1), [Why::Comment { by: colleague() }]);
    world.finish();
}

#[test]
fn a_review_whose_diff_is_the_same_at_the_new_head_waits_on_nobody() {
    let world = World::new("attention-not-stale");
    world.reviewed_and_moved(reviewed_by_me(1), 'a', 'a');

    assert_eq!(world.why(1), []);
    assert_eq!(world.workbench().count, 0);
    world.finish();
}

#[test]
fn a_stale_review_of_a_colleagues_draft_waits_for_a_review_request() {
    let world = World::new("attention-stale-draft");
    let draft = PullRequestFacts {
        is_draft: true,
        ..reviewed_by_me(1)
    };
    world.reviewed_and_moved(draft, 'a', 'b');

    assert_eq!(world.why(1), []);
    world.finish();
}

#[test]
fn a_stale_review_of_a_merged_pr_waits_on_nobody() {
    let world = World::new("attention-stale-merged");
    let merged = PullRequestFacts {
        state: PrState::Merged,
        ..reviewed_by_me(1)
    };
    world.reviewed_and_moved(merged, 'a', 'b');

    assert_eq!(world.workbench().count, 0);
    world.finish();
}

impl World {
    /// The review state of PR `number` in the tree of Issue 10, which PR 2's Review request keeps on the
    /// Workbench.
    fn review_in_issue(&self, number: u32) -> YourReview {
        let workbench = self.workbench();
        let WorkbenchItem::Issue(issue) = listed_at(&workbench, 10).item else {
            panic!("Issue 10 isn't on the Workbench");
        };
        let mut rows = issue.pull_requests.iter().flat_map(TreeEntry::rows);
        let row = rows.find(|row| row.number == number).unwrap();
        row.your_review.clone()
    }
}

#[test]
fn a_merged_prs_stale_review_keeps_its_files_and_adds_nothing_to_needs_attention() {
    let world = World::new("attention-stale-merged-row");
    world.has_issues(vec![issue(10)]);
    let merged = PullRequestFacts {
        state: PrState::Merged,
        ..reviewed_by_me(1)
    };
    world.reviewed_and_moved(fixing_too(merged, 10), 'a', 'b');
    world.has(vec![fixing_too(requested(2), 10)]);

    assert_eq!(
        world.review_in_issue(1),
        YourReview::StaleReview {
            reviewed: sha_of('1'),
            head: sha_of('2'),
            files: vec![ChangedFile {
                path: "Lib/ast.py".to_owned(),
                whitespace_only: false,
            }],
        }
    );
    assert_eq!(world.why(10), [Why::ReviewRequested]);
    world.finish();
}

#[test]
fn a_closed_pr_whose_check_cant_run_shows_why_and_waits_on_nobody() {
    let world = World::new("attention-unchecked-closed");
    world.has_issues(vec![issue(10)]);
    let closed = PullRequestFacts {
        state: PrState::Closed,
        ..reviewed_by_me(1)
    };
    world.reviewed_and_moved_unread(fixing_too(closed, 10));
    world.has(vec![fixing_too(requested(2), 10)]);
    let refused = UncheckedCommit {
        of: on_main(sha_of('1')),
        reason: CantCheck::Refused,
    };
    world
        .db
        .store
        .save_unchecked(&cpython(), vec![refused])
        .unwrap();

    assert_eq!(
        world.review_in_issue(1),
        YourReview::CantCheck {
            reviewed: sha_of('1'),
            reason: CantCheck::Refused
        }
    );
    assert_eq!(world.why(10), [Why::ReviewRequested]);
    world.finish();
}
