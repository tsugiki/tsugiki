//! Each rule of Needs attention as a scenario in the glossary's words, read the way the window reads it.

use std::fs;
use std::path::PathBuf;

use time::{Duration, OffsetDateTime};

use super::{Attention, Why};
use crate::github::activity::{Activity, ActivityKind, ItemActivity};
use crate::github::issues::{IssueFacts, IssueState};
use crate::github::pull_requests::{CiState, LatestReview, PrState, PullRequestFacts, ReviewState};
use crate::login::Login;
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;
use crate::store::{ChangeCheck, OpenReader, Store, StoreReader, SyncState, reader};
use crate::workbench::{ListedItem, Workbench, WorkbenchView};

fn me() -> Login {
    Login::new("octocat")
}

fn colleague() -> Login {
    Login::new("some-author")
}

fn reviewer() -> Login {
    Login::new("some-reviewer")
}

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn hours_ago(hours: i64) -> OffsetDateTime {
    let at = OffsetDateTime::now_utc() - Duration::hours(hours);
    OffsetDateTime::from_unix_timestamp(at.unix_timestamp()).unwrap()
}

/// A colleague's open PR that fixes no Issue, and has nothing to do with the user.
fn pull_request(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("Fix it {number}"),
        state: PrState::Open,
        is_draft: false,
        author: Some(colleague()),
        target_branch: "main".to_owned(),
        ci: None,
        updated_at: hours_ago(100),
        review_requests: vec![],
        latest_reviews: vec![],
        assignees: vec![],
        closing_issues: vec![],
        merge_commit: None,
        base: None,
        head: None,
        off_default: None,
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

fn requested(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        review_requests: vec![me()],
        ..pull_request(number)
    }
}

/// A PR that fixes Issue `issue`, which the title says.
fn fixing(number: u32, issue: u32) -> PullRequestFacts {
    PullRequestFacts {
        title: format!("gh-{issue}: Fix it"),
        ..pull_request(number)
    }
}

fn issue(number: u32) -> IssueFacts {
    IssueFacts {
        number,
        title: format!("Issue {number}"),
        author: Some(Login::new("issue-author")),
        assignees: vec![],
        updated_at: hours_ago(100),
        state: Some(IssueState::Open),
    }
}

fn comment(who: &Login, at: OffsetDateTime) -> Activity {
    Activity {
        kind: ActivityKind::Comment,
        who: who.clone(),
        at,
    }
}

/// A Repository whose first sync began three days ago and whose user signed in as `me()`.
struct World {
    store: Store,
    reader: StoreReader,
    open_reader: OpenReader,
    folder: PathBuf,
    settings: RepositorySettings,
}

impl World {
    fn new(name: &str) -> Self {
        let folder = crate::testing::temp_path(&format!("attention-{name}"));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("tsugiki.db");
        let store = Store::open(&path).unwrap();
        store.remember_login(&me()).unwrap();
        let started_at = hours_ago(72);
        let state = SyncState {
            started_at,
            finished_at: started_at,
            check: ChangeCheck::Skipped,
            refresh_under_way: false,
        };
        store.save_sync_state(&cpython(), state).unwrap();
        let (to_open, reader) = reader();
        let open_reader = to_open.open(&path).unwrap();
        Self {
            store,
            reader,
            open_reader,
            folder,
            settings: RepositorySettings::new(cpython()),
        }
    }

    fn has(&self, pull_requests: Vec<PullRequestFacts>) {
        self.store
            .save_pull_requests(&cpython(), pull_requests)
            .unwrap();
    }

    fn has_issues(&self, issues: Vec<IssueFacts>) {
        self.store.save_issues(&cpython(), issues).unwrap();
    }

    fn happened(&self, number: u32, events: Vec<Activity>) {
        let activity = vec![ItemActivity { number, events }];
        self.store
            .save_fetched_pull_requests(&cpython(), vec![], activity)
            .unwrap();
    }

    fn opened(&self, number: u32, at: OffsetDateTime) {
        self.store.mark_seen(&cpython(), number, at).unwrap();
    }

    fn view(&self, view: &WorkbenchView) -> Workbench {
        Workbench::read(&self.reader, &self.settings, view).unwrap()
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

    fn since(&self, number: u32) -> OffsetDateTime {
        let workbench = self.workbench();
        let attention: Attention = listed_at(&workbench, number).attention.unwrap();
        attention.since
    }

    fn finish(self) {
        self.open_reader.close();
        self.store.close();
        fs::remove_dir_all(self.folder).unwrap();
    }
}

fn number_of(listed: &ListedItem) -> u32 {
    listed.item.updated_at_and_number().1
}

fn listed_at(workbench: &Workbench, number: u32) -> ListedItem {
    let all = workbench.items.iter().chain(&workbench.quiet);
    let mut found = all.filter(|listed| number_of(listed) == number);
    found.next().unwrap().clone()
}

fn numbers(list: &[ListedItem]) -> Vec<u32> {
    list.iter().map(number_of).collect()
}

#[test]
fn a_review_request_needs_attention_from_when_it_was_asked() {
    let world = World::new("review-request");
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
    let world = World::new("draft-request");
    world.has(vec![PullRequestFacts {
        is_draft: true,
        ..requested(1)
    }]);

    assert_eq!(world.why(1), [Why::ReviewRequested]);
    world.finish();
}

#[test]
fn opening_a_pr_with_a_review_request_leaves_it_waiting() {
    let world = World::new("request-stays");
    world.has(vec![requested(1)]);
    world.opened(1, hours_ago(1));

    assert_eq!(world.why(1), [Why::ReviewRequested]);
    world.finish();
}

#[test]
fn a_review_request_with_no_event_left_on_the_timeline_counts_from_the_last_update() {
    let world = World::new("request-no-event");
    let updated_at = hours_ago(9);
    world.has(vec![PullRequestFacts {
        updated_at,
        ..requested(1)
    }]);

    assert_eq!(world.since(1), updated_at);
    world.finish();
}

#[test]
fn someone_elses_review_request_is_no_time_for_the_users() {
    let world = World::new("request-other");
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
    let world = World::new("assignee");
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
    let world = World::new("self-assigned");
    world.has(vec![PullRequestFacts {
        assignees: vec![me()],
        ..own(1)
    }]);

    assert_eq!(world.why(1), []);
    assert_eq!(world.workbench().review_requests, 0);
    world.finish();
}

#[test]
fn failed_ci_on_an_own_pr_needs_attention_and_stays_after_opening_it() {
    let world = World::new("own-ci");
    world.has(vec![PullRequestFacts {
        ci: Some(CiState::Failure),
        ..own(1)
    }]);
    world.opened(1, OffsetDateTime::now_utc() + Duration::hours(1));

    assert_eq!(world.why(1), [Why::CiFailed { checks: vec![] }]);
    world.finish();
}

#[test]
fn failed_ci_names_the_checks_once_tsugiki_has_asked() {
    let world = World::new("own-ci-names");
    world.has(vec![PullRequestFacts {
        ci: Some(CiState::Failure),
        ..own(1)
    }]);
    let names = vec!["Tests / Ubuntu".to_owned()];
    world
        .store
        .save_failed_checks(&cpython(), vec![(1, names.clone())])
        .unwrap();

    assert_eq!(world.why(1), [Why::CiFailed { checks: names }]);
    world.finish();
}

#[test]
fn failed_ci_on_a_merged_own_pr_waits_on_nobody() {
    let world = World::new("merged-ci");
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
    let world = World::new("colleague-ci");
    world.has(vec![PullRequestFacts {
        ci: Some(CiState::Failure),
        ..pull_request(1)
    }]);

    assert_eq!(world.workbench().items, []);
    world.finish();
}

#[test]
fn a_passing_run_on_an_own_pr_is_new_activity_until_it_is_opened() {
    let world = World::new("own-ci-pass");
    world.has(vec![PullRequestFacts {
        ci: Some(CiState::Success),
        ..own(1)
    }]);
    assert_eq!(world.why(1), [Why::CiPassed]);

    world.opened(1, OffsetDateTime::now_utc() + Duration::hours(1));

    assert_eq!(world.why(1), []);
    world.finish();
}

#[test]
fn new_activity_on_a_pr_the_user_wrote_or_reviewed_needs_attention() {
    let world = World::new("wrote-or-reviewed");
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
    let world = World::new("review-words");
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
    let world = World::new("backport-conflict");
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
    let world = World::new("unreviewed");
    world.has(vec![pull_request(1)]);
    world.happened(1, vec![comment(&reviewer(), hours_ago(1))]);

    assert_eq!(world.workbench().items, []);
    world.finish();
}

#[test]
fn a_colleagues_draft_with_new_activity_waits_for_a_review_request() {
    let world = World::new("draft");
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
    let world = World::new("pr-conversation");
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
    let world = World::new("pr-mention");
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
    let world = World::new("issue-conversation");
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
fn a_mention_in_a_new_post_on_an_issue_needs_attention() {
    let world = World::new("issue-mention");
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
    let world = World::new("issue-only-follows");
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
    assert_eq!(world.workbench().quiet, []);
    world.finish();
}

#[test]
fn an_issue_needs_attention_when_a_pr_in_its_tree_does_and_says_why_with_the_newest_first() {
    let world = World::new("tree");
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
    let world = World::new("said-once");
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
fn what_needs_attention_comes_first_newest_first_then_the_rest_then_the_quiet_section() {
    let world = World::new("order");
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

    assert_eq!(numbers(&workbench.items), [2, 1, 3, 4]);
    assert_eq!(numbers(&workbench.quiet), [10, 11]);
    world.finish();
}

#[test]
fn the_sidebar_counts_what_needs_attention_and_review_requests_lists_those_prs() {
    let world = World::new("counts");
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
    assert_eq!(requests.quiet, []);
    assert_eq!(requests.count, 3);
    world.finish();
}
