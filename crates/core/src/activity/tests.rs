//! Each rule of New activity as a scenario in the glossary's words.

use std::fs;
use std::path::PathBuf;

use time::{Duration, OffsetDateTime};

use super::{NewActivity, News, WhatHappened};
use crate::conventions::{CPYTHON_BACKPORT_BOT, Conventions};
use crate::github::Login;
use crate::github::activity::{Activity, ActivityKind, ItemActivity};
use crate::github::issues::{IssueFacts, IssueState};
use crate::github::pull_requests::{
    CiState, LatestReview, OffDefault, PrState, PullRequestFacts, ReviewState,
};
use crate::repository::RepositoryName;
use crate::store::{ChangeCheck, OpenReader, Store, StoreReader, SyncState, WorkFacts, reader};

const OWN_PR: u32 = 1;
const COLLEAGUES_PR: u32 = 2;
const ISSUE: u32 = 3;
const BOT_BACKPORT: u32 = 4;

fn me() -> Login {
    Login::new("octocat")
}

fn colleague() -> Login {
    Login::new("some-author")
}

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn hours_ago(hours: i64) -> OffsetDateTime {
    OffsetDateTime::now_utc() - Duration::hours(hours)
}

fn pull_request(number: u32, author: &Login) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("gh-1: Fix it {number}"),
        state: PrState::Open,
        is_draft: false,
        author: Some(author.clone()),
        target_branch: "main".to_owned(),
        ci: None,
        updated_at: hours_ago(100),
        review_requests: vec![],
        latest_reviews: vec![],
        assignees: vec![],
        closing_issues: vec![],
        merge_commit: None,
        head: None,
        off_default: None,
    }
}

/// A Backport of `OWN_PR` under `author`, as the link rules read one.
fn backport(number: u32, author: &str) -> PullRequestFacts {
    let author = Login::new(author);
    PullRequestFacts {
        title: format!("[3.14] gh-1: Fix it (GH-{OWN_PR})"),
        target_branch: "3.14".to_owned(),
        off_default: Some(OffDefault {
            committers: vec![Some(author.clone())],
            ..OffDefault::described("")
        }),
        ..pull_request(number, &author)
    }
}

fn event(kind: ActivityKind, who: &Login, at: OffsetDateTime) -> Activity {
    Activity {
        kind,
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
}

impl World {
    fn new(name: &str) -> Self {
        let world = Self::before_its_first_sync(name);
        world.finishes_its_first_sync();
        world
    }

    fn finishes_its_first_sync(&self) {
        let started_at = hours_ago(72);
        let state = SyncState {
            started_at,
            finished_at: started_at,
            check: ChangeCheck::Skipped,
            refresh_under_way: false,
        };
        self.store.save_sync_state(&cpython(), state).unwrap();
    }

    fn before_its_first_sync(name: &str) -> Self {
        let folder = crate::testing::temp_path(&format!("activity-{name}"));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("tsugiki.db");
        let store = Store::open(&path).unwrap();
        store.remember_login(&me()).unwrap();
        let (to_open, reader) = reader();
        let open_reader = to_open.open(&path).unwrap();
        Self {
            store,
            reader,
            open_reader,
            folder,
        }
    }

    fn has(&self, pull_requests: Vec<PullRequestFacts>) {
        self.store
            .save_pull_requests(&cpython(), pull_requests)
            .unwrap();
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

    /// CI on `pull_request` reaches `result`, and a sync saves it.
    fn ci_reaches(&self, pull_request: &PullRequestFacts, result: CiState) {
        let finished = PullRequestFacts {
            ci: Some(result),
            ..pull_request.clone()
        };
        self.has(vec![finished]);
    }

    fn facts(&self) -> WorkFacts {
        self.reader.work_facts(&cpython()).unwrap()
    }

    /// The facts with every event on item `number`. The Workbench's read leaves out items that don't concern
    /// the user, and New activity says what happened on them too.
    fn facts_about(&self, number: u32) -> WorkFacts {
        let mut facts = self.facts();
        let events = self.store.activity(&cpython(), number).unwrap();
        let first_sync = facts.activity.first_sync;
        let later = events
            .into_iter()
            .filter(|event| first_sync.is_some_and(|started| event.at > started));
        let events = facts.activity.events.entry(number).or_default();
        events.clear();
        events.extend(later);
        facts
    }

    fn new_on_pull_request(&self, number: u32) -> Vec<WhatHappened> {
        let facts = self.facts_about(number);
        let held = &facts.pull_requests;
        let viewer = facts.viewer.as_ref().unwrap();
        let news = News::new(&facts.activity, held, viewer, Conventions::Cpython);
        let pull_request = held.iter().find(|pr| pr.number == number).unwrap();
        let news = news.on_pull_request(pull_request);
        news.into_iter().map(|new: NewActivity| new.what).collect()
    }

    fn new_on_issue(&self, number: u32) -> Vec<WhatHappened> {
        let facts = self.facts_about(number);
        let viewer = facts.viewer.as_ref().unwrap();
        let news = News::new(&facts.activity, &[], viewer, Conventions::Cpython);
        let news = news.on_issue(number);
        news.into_iter().map(|new: NewActivity| new.what).collect()
    }

    fn finish(self) {
        self.open_reader.close();
        self.store.close();
        fs::remove_dir_all(self.folder).unwrap();
    }
}

#[test]
fn someone_elses_comment_after_last_seen_is_new_activity() {
    let world = World::new("comment-after");
    world.has(vec![pull_request(COLLEAGUES_PR, &colleague())]);
    world.opened(COLLEAGUES_PR, hours_ago(2));
    let comment = event(ActivityKind::Comment, &colleague(), hours_ago(1));
    world.happened(COLLEAGUES_PR, vec![comment]);

    assert_eq!(
        world.new_on_pull_request(COLLEAGUES_PR),
        [WhatHappened::Comment(colleague())]
    );
    world.finish();
}

#[test]
fn opening_a_pr_clears_what_happened_before_it() {
    let world = World::new("comment-before");
    world.has(vec![pull_request(COLLEAGUES_PR, &colleague())]);
    let comment = event(ActivityKind::Comment, &colleague(), hours_ago(2));
    world.happened(COLLEAGUES_PR, vec![comment]);
    world.opened(COLLEAGUES_PR, hours_ago(1));

    assert_eq!(world.new_on_pull_request(COLLEAGUES_PR), []);
    world.finish();
}

#[test]
fn peoples_commits_reviews_and_a_draft_turning_ready_are_new_activity_in_the_order_tsugiki_knew_them()
 {
    let world = World::new("kinds");
    world.has(vec![pull_request(COLLEAGUES_PR, &colleague())]);
    world.opened(COLLEAGUES_PR, hours_ago(5));
    let reviewer = Login::new("some-reviewer");
    world.happened(
        COLLEAGUES_PR,
        vec![
            event(ActivityKind::Review, &reviewer, hours_ago(1)),
            event(ActivityKind::Commit, &colleague(), hours_ago(3)),
            event(ActivityKind::ReadyForReview, &colleague(), hours_ago(2)),
        ],
    );

    assert_eq!(
        world.new_on_pull_request(COLLEAGUES_PR),
        [
            WhatHappened::ReadyForReview(colleague()),
            WhatHappened::Review(reviewer),
            WhatHappened::Commit(colleague()),
        ]
    );
    world.finish();
}

#[test]
fn what_the_user_did_isnt_new_activity() {
    let world = World::new("own");
    world.has(vec![pull_request(COLLEAGUES_PR, &colleague())]);
    world.opened(COLLEAGUES_PR, hours_ago(5));
    let at = hours_ago(1);
    world.happened(
        COLLEAGUES_PR,
        vec![
            event(ActivityKind::Commit, &me(), at),
            event(ActivityKind::Comment, &me(), at),
            event(ActivityKind::Review, &me(), at),
            event(ActivityKind::ReadyForReview, &me(), at),
        ],
    );

    assert_eq!(world.new_on_pull_request(COLLEAGUES_PR), []);
    world.finish();
}

#[test]
fn a_bot_that_github_takes_for_a_person_isnt_new_activity() {
    let world = World::new("bot-account");
    world.has(vec![pull_request(COLLEAGUES_PR, &colleague())]);
    world.opened(COLLEAGUES_PR, hours_ago(5));
    let at = hours_ago(1);
    world.happened(
        COLLEAGUES_PR,
        vec![
            event(ActivityKind::Comment, &Login::new("bedevere-bot"), at),
            event(ActivityKind::Commit, &Login::new(CPYTHON_BACKPORT_BOT), at),
        ],
    );

    assert_eq!(world.new_on_pull_request(COLLEAGUES_PR), []);
    world.finish();
}

#[test]
fn a_label_change_moves_a_pr_on_github_and_isnt_new_activity() {
    let world = World::new("label");
    world.has(vec![pull_request(COLLEAGUES_PR, &colleague())]);
    world.opened(COLLEAGUES_PR, hours_ago(5));
    let labelled = PullRequestFacts {
        updated_at: hours_ago(1),
        ..pull_request(COLLEAGUES_PR, &colleague())
    };
    world.has(vec![labelled]);

    assert_eq!(world.new_on_pull_request(COLLEAGUES_PR), []);
    world.finish();
}

#[test]
fn ci_finishing_on_an_own_pr_is_new_activity() {
    let world = World::new("ci-own");
    let own = PullRequestFacts {
        ci: Some(CiState::Pending),
        ..pull_request(OWN_PR, &me())
    };
    world.has(vec![own.clone()]);
    world.opened(OWN_PR, hours_ago(1));

    world.ci_reaches(&own, CiState::Failure);

    assert_eq!(
        world.new_on_pull_request(OWN_PR),
        [WhatHappened::CiFinished(CiState::Failure)]
    );
    world.finish();
}

#[test]
fn ci_finishing_on_a_pr_the_user_has_a_review_on_is_new_activity() {
    let world = World::new("ci-reviewed");
    let reviewed = PullRequestFacts {
        ci: Some(CiState::Failure),
        latest_reviews: vec![LatestReview {
            author: me(),
            state: ReviewState::Approved,
        }],
        ..pull_request(COLLEAGUES_PR, &colleague())
    };
    world.has(vec![reviewed.clone()]);
    world.opened(COLLEAGUES_PR, hours_ago(1));

    world.ci_reaches(&reviewed, CiState::Success);

    assert_eq!(
        world.new_on_pull_request(COLLEAGUES_PR),
        [WhatHappened::CiFinished(CiState::Success)]
    );
    world.finish();
}

#[test]
fn ci_on_someone_elses_pr_the_user_hasnt_reviewed_isnt_new_activity() {
    let world = World::new("ci-other");
    let theirs = pull_request(COLLEAGUES_PR, &colleague());
    world.has(vec![theirs.clone()]);
    world.opened(COLLEAGUES_PR, hours_ago(1));

    world.ci_reaches(&theirs, CiState::Failure);

    assert_eq!(world.new_on_pull_request(COLLEAGUES_PR), []);
    world.finish();
}

#[test]
fn ci_going_back_to_pending_after_a_push_isnt_new_activity() {
    let world = World::new("ci-pending");
    let own = PullRequestFacts {
        ci: Some(CiState::Success),
        ..pull_request(OWN_PR, &me())
    };
    world.has(vec![own.clone()]);
    world.opened(OWN_PR, hours_ago(1));

    world.ci_reaches(&own, CiState::Pending);

    assert_eq!(world.new_on_pull_request(OWN_PR), []);
    world.finish();
}

#[test]
fn ci_that_finished_before_last_seen_isnt_new_activity() {
    let world = World::new("ci-seen");
    let own = pull_request(OWN_PR, &me());
    world.has(vec![own.clone()]);
    world.ci_reaches(&own, CiState::Success);

    world.opened(OWN_PR, OffsetDateTime::now_utc() + Duration::hours(1));

    assert_eq!(world.new_on_pull_request(OWN_PR), []);
    world.finish();
}

#[test]
fn ci_a_first_sync_finds_isnt_new_activity() {
    let world = World::before_its_first_sync("ci-first-sync");
    let own = PullRequestFacts {
        ci: Some(CiState::Failure),
        ..pull_request(OWN_PR, &me())
    };
    world.has(vec![own]);
    world.finishes_its_first_sync();

    assert_eq!(world.new_on_pull_request(OWN_PR), []);
    world.finish();
}

#[test]
fn ci_on_an_own_pr_tsugiki_first_sees_after_the_first_sync_is_new_activity() {
    let world = World::new("ci-new-pr");
    let own = PullRequestFacts {
        ci: Some(CiState::Failure),
        ..pull_request(OWN_PR, &me())
    };
    world.has(vec![own]);

    assert_eq!(
        world.new_on_pull_request(OWN_PR),
        [WhatHappened::CiFinished(CiState::Failure)]
    );
    world.finish();
}

#[test]
fn a_review_the_user_hasnt_submitted_doesnt_make_ci_news() {
    let world = World::new("ci-pending-review");
    let drafted = PullRequestFacts {
        latest_reviews: vec![LatestReview {
            author: me(),
            state: ReviewState::Pending,
        }],
        ..pull_request(COLLEAGUES_PR, &colleague())
    };
    world.has(vec![drafted.clone()]);
    world.opened(COLLEAGUES_PR, hours_ago(1));

    world.ci_reaches(&drafted, CiState::Failure);

    assert_eq!(world.new_on_pull_request(COLLEAGUES_PR), []);
    world.finish();
}

#[test]
fn a_commit_made_before_last_seen_and_pushed_after_it_is_new_activity() {
    let world = World::new("commit-pushed-later");
    world.has(vec![pull_request(COLLEAGUES_PR, &colleague())]);
    world.opened(COLLEAGUES_PR, hours_ago(1));

    let made_earlier = event(ActivityKind::Commit, &colleague(), hours_ago(2));
    world.happened(COLLEAGUES_PR, vec![made_earlier]);

    assert_eq!(
        world.new_on_pull_request(COLLEAGUES_PR),
        [WhatHappened::Commit(colleague())]
    );
    world.finish();
}

#[test]
fn a_commit_tsugiki_already_read_stays_seen_when_github_lists_it_again() {
    let world = World::new("commit-listed-again");
    world.has(vec![pull_request(COLLEAGUES_PR, &colleague())]);
    let commit = event(ActivityKind::Commit, &colleague(), hours_ago(2));
    world.happened(COLLEAGUES_PR, vec![commit.clone()]);
    world.opened(
        COLLEAGUES_PR,
        OffsetDateTime::now_utc() + Duration::hours(1),
    );

    world.happened(COLLEAGUES_PR, vec![commit]);

    assert_eq!(world.new_on_pull_request(COLLEAGUES_PR), []);
    world.finish();
}

#[test]
fn the_old_commits_of_a_pr_tsugiki_first_sees_after_the_first_sync_arent_new_activity() {
    let world = World::new("commit-first-sight");
    let old = event(ActivityKind::Commit, &colleague(), hours_ago(100));
    let activity = vec![ItemActivity {
        number: COLLEAGUES_PR,
        events: vec![old],
    }];
    let theirs = pull_request(COLLEAGUES_PR, &colleague());
    world
        .store
        .save_fetched_pull_requests(&cpython(), vec![theirs], activity)
        .unwrap();

    assert_eq!(world.new_on_pull_request(COLLEAGUES_PR), []);
    world.finish();
}

#[test]
fn a_bot_backport_tsugiki_first_sees_with_failed_ci_is_new_activity_on_its_origin_pr() {
    let world = World::new("backport-first-sight");
    world.has(vec![pull_request(OWN_PR, &me())]);
    world.opened(OWN_PR, hours_ago(1));

    let failed = PullRequestFacts {
        ci: Some(CiState::Failure),
        ..backport(BOT_BACKPORT, CPYTHON_BACKPORT_BOT)
    };
    world.has(vec![failed]);

    assert_eq!(
        world.new_on_pull_request(OWN_PR),
        [WhatHappened::BackportCiFailed {
            number: BOT_BACKPORT,
            target: "3.14".to_owned(),
        }]
    );
    world.finish();
}

#[test]
fn one_failure_on_a_bot_backport_the_user_reviewed_is_one_item() {
    let world = World::new("backport-reviewed");
    let reviewed = PullRequestFacts {
        latest_reviews: vec![LatestReview {
            author: me(),
            state: ReviewState::Approved,
        }],
        ..backport(BOT_BACKPORT, CPYTHON_BACKPORT_BOT)
    };
    world.has(vec![pull_request(OWN_PR, &me()), reviewed.clone()]);
    world.opened(BOT_BACKPORT, hours_ago(1));

    world.ci_reaches(&reviewed, CiState::Failure);

    assert_eq!(
        world.new_on_pull_request(BOT_BACKPORT),
        [WhatHappened::BackportCiFailed {
            number: BOT_BACKPORT,
            target: "3.14".to_owned(),
        }]
    );
    world.finish();
}

#[test]
fn the_bot_failing_to_make_a_backport_is_new_activity_on_the_origin_pr() {
    let world = World::new("backport-conflict");
    world.has(vec![pull_request(OWN_PR, &me())]);
    world.opened(OWN_PR, hours_ago(2));
    let failed = ActivityKind::BackportFailed {
        target: "3.14".to_owned(),
    };
    let bot = Login::new("miss-islington-app");
    world.happened(OWN_PR, vec![event(failed, &bot, hours_ago(1))]);

    assert_eq!(
        world.new_on_pull_request(OWN_PR),
        [WhatHappened::BackportConflict {
            target: "3.14".to_owned()
        }]
    );
    world.finish();
}

#[test]
fn a_bot_backport_whose_ci_fails_is_new_activity_on_it_and_on_its_origin_pr() {
    let world = World::new("backport-ci");
    let bot_backport = backport(BOT_BACKPORT, CPYTHON_BACKPORT_BOT);
    world.has(vec![pull_request(OWN_PR, &me()), bot_backport.clone()]);
    world.opened(OWN_PR, hours_ago(1));
    world.opened(BOT_BACKPORT, hours_ago(1));

    world.ci_reaches(&bot_backport, CiState::Failure);

    let failed = [WhatHappened::BackportCiFailed {
        number: BOT_BACKPORT,
        target: "3.14".to_owned(),
    }];
    assert_eq!(world.new_on_pull_request(OWN_PR), failed);
    assert_eq!(world.new_on_pull_request(BOT_BACKPORT), failed);
    world.finish();
}

#[test]
fn a_bot_backport_whose_ci_passes_isnt_a_failed_bot_backport() {
    let world = World::new("backport-ci-passed");
    let bot_backport = backport(BOT_BACKPORT, CPYTHON_BACKPORT_BOT);
    world.has(vec![pull_request(OWN_PR, &me()), bot_backport.clone()]);
    world.opened(OWN_PR, hours_ago(1));

    world.ci_reaches(&bot_backport, CiState::Success);

    assert_eq!(world.new_on_pull_request(OWN_PR), []);
    world.finish();
}

#[test]
fn a_failed_bot_backport_seen_on_its_origin_pr_is_no_longer_new_there() {
    let world = World::new("backport-ci-seen");
    let bot_backport = backport(BOT_BACKPORT, CPYTHON_BACKPORT_BOT);
    world.has(vec![pull_request(OWN_PR, &me()), bot_backport.clone()]);
    world.ci_reaches(&bot_backport, CiState::Error);

    world.opened(OWN_PR, OffsetDateTime::now_utc() + Duration::hours(1));

    assert_eq!(world.new_on_pull_request(OWN_PR), []);
    world.finish();
}

#[test]
fn a_manual_backport_whose_ci_fails_isnt_a_failed_bot_backport() {
    let world = World::new("manual-backport-ci");
    let manual = backport(BOT_BACKPORT, "some-author");
    world.has(vec![pull_request(OWN_PR, &me()), manual.clone()]);
    world.opened(OWN_PR, hours_ago(1));

    world.ci_reaches(&manual, CiState::Failure);

    assert_eq!(world.new_on_pull_request(OWN_PR), []);
    world.finish();
}

#[test]
fn a_pr_never_opened_shows_what_happened_since_the_first_sync_and_nothing_older() {
    let world = World::new("never-opened");
    world.has(vec![pull_request(COLLEAGUES_PR, &colleague())]);
    let older = event(ActivityKind::Comment, &colleague(), hours_ago(100));
    let since = event(ActivityKind::Review, &colleague(), hours_ago(1));
    world.happened(COLLEAGUES_PR, vec![older, since]);

    assert_eq!(
        world.new_on_pull_request(COLLEAGUES_PR),
        [WhatHappened::Review(colleague())]
    );
    world.finish();
}

#[test]
fn nothing_is_new_before_the_first_sync_finishes() {
    let world = World::before_its_first_sync("no-first-sync");
    world.has(vec![pull_request(COLLEAGUES_PR, &colleague())]);
    let comment = event(ActivityKind::Comment, &colleague(), hours_ago(1));
    world.happened(COLLEAGUES_PR, vec![comment.clone()]);
    world.happened(ISSUE, vec![comment]);

    assert_eq!(world.new_on_pull_request(COLLEAGUES_PR), []);
    assert_eq!(world.new_on_issue(ISSUE), []);
    world.finish();
}

#[test]
fn a_later_refresh_doesnt_move_the_start_of_news() {
    let world = World::new("refresh-keeps-start");
    let started_at = world.facts().activity.first_sync;
    let later = SyncState {
        started_at: hours_ago(1),
        finished_at: hours_ago(1),
        check: ChangeCheck::Skipped,
        refresh_under_way: false,
    };
    world.store.save_sync_state(&cpython(), later).unwrap();

    assert_eq!(world.facts().activity.first_sync, started_at);
    world.finish();
}

#[test]
fn someone_elses_comment_on_an_issue_is_new_activity() {
    let world = World::new("issue");
    let issue = IssueFacts {
        number: ISSUE,
        title: "An Issue".to_owned(),
        author: Some(colleague()),
        assignees: vec![],
        updated_at: hours_ago(1),
        state: Some(IssueState::Open),
    };
    let activity = vec![ItemActivity {
        number: ISSUE,
        events: vec![
            event(ActivityKind::Comment, &colleague(), hours_ago(1)),
            event(ActivityKind::Comment, &me(), hours_ago(1)),
        ],
    }];
    world
        .store
        .save_fetched_issues(&cpython(), vec![issue], activity)
        .unwrap();
    world.opened(ISSUE, hours_ago(2));

    assert_eq!(
        world.new_on_issue(ISSUE),
        [WhatHappened::Comment(colleague())]
    );
    world.finish();
}

#[test]
fn last_seen_doesnt_move_back_when_the_clock_does() {
    let world = World::new("clock");
    let later = hours_ago(1).replace_nanosecond(0).unwrap();
    world.opened(ISSUE, later);
    world.opened(ISSUE, hours_ago(5));

    assert_eq!(world.facts().activity.last_seen.get(&ISSUE), Some(&later));
    world.finish();
}

#[test]
fn a_later_answer_listing_the_same_events_keeps_one_of_each() {
    let world = World::new("same-events");
    let at = hours_ago(1).replace_nanosecond(0).unwrap();
    let comment = event(ActivityKind::Comment, &colleague(), at);
    world.happened(ISSUE, vec![comment.clone()]);
    world.happened(ISSUE, vec![comment.clone()]);

    assert_eq!(world.store.activity(&cpython(), ISSUE).unwrap(), [comment]);
    world.finish();
}

#[test]
fn each_kind_of_event_reads_back_as_it_was_saved() {
    let world = World::new("round-trip");
    let at = hours_ago(1).replace_nanosecond(0).unwrap();
    let failed = ActivityKind::BackportFailed {
        target: "3.14".to_owned(),
    };
    let events: Vec<Activity> = [
        ActivityKind::Commit,
        ActivityKind::Comment,
        ActivityKind::Review,
        ActivityKind::ReadyForReview,
        failed,
    ]
    .into_iter()
    .map(|kind| event(kind, &colleague(), at))
    .collect();
    world.happened(ISSUE, events.clone());

    let mut read = world.store.activity(&cpython(), ISSUE).unwrap();
    read.sort_by(|left, right| left.kind.cmp(&right.kind));
    assert_eq!(read, events);
    world.finish();
}

#[test]
fn a_failed_bot_backport_nobody_can_act_on_any_more_is_no_news() {
    let world = World::new("closed-backport");
    let own = pull_request(OWN_PR, &me());
    let closed = PullRequestFacts {
        state: PrState::Closed,
        ci: Some(CiState::Failure),
        ..backport(BOT_BACKPORT, CPYTHON_BACKPORT_BOT)
    };
    world.has(vec![own, closed]);

    assert_eq!(world.new_on_pull_request(OWN_PR), []);
    world.finish();
}
