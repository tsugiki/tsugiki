//! A PR's review state on its row, from the user's Review and the patch-ids of both sides.

use std::collections::BTreeMap;

use crate::github::CommitSha;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::{LatestReview, PrState, PullRequestFacts, ReviewState};
use crate::login::Login;
use crate::patch_ids::{CantCheck, ChangedFile, FilePatch, PatchId, PatchesOf};
use crate::store::{ActivityFacts, LastCheck, ReviewFacts, WorkFacts};
use crate::workbench::attention::Causes;
use crate::workbench::testing::{
    BOT, at, backport, bots_backport, cpython, issue, me, outline, own, reviewed_workbench,
    someone, stranger,
};
use crate::workbench::{Why, Workbench, WorkbenchItem, WorkbenchView, YourReview};

fn sha(digit: char) -> CommitSha {
    CommitSha::parse(&digit.to_string().repeat(40)).unwrap()
}

fn file(path: &str, verbatim: char, loose: char) -> FilePatch {
    let id = |digit: char| PatchId::parse(&digit.to_string().repeat(40)).unwrap();
    FilePatch {
        path: path.to_owned(),
        verbatim: id(verbatim),
        loose: id(loose),
        generated: false,
    }
}

/// A colleague's PR 1 at commit 2, which the user reviewed.
fn reviewed_pr() -> PullRequestFacts {
    PullRequestFacts {
        head: Some(sha('2')),
        latest_reviews: vec![LatestReview {
            author: me(),
            state: ReviewState::Approved,
        }],
        ..stranger()
    }
}

/// The user's Review of PR 1 at commit 1, and the patch-ids read of each side.
fn reviewed_at_1(reviewed: Option<Vec<FilePatch>>, head: Option<Vec<FilePatch>>) -> ReviewFacts {
    let of = |digit: char| PatchesOf {
        commit: sha(digit),
        target_branch: "main".to_owned(),
    };
    let sides = [(of('1'), reviewed), (of('2'), head)];
    ReviewFacts {
        reviewed: BTreeMap::from([(1, sha('1'))]),
        patches: sides
            .into_iter()
            .filter_map(|(of, files)| Some((of, files?)))
            .collect(),
        ..ReviewFacts::default()
    }
}

/// The review state on PR 1's row. A Review request asks for the user's Review again, which keeps the PR on
/// the Workbench whatever else it has to do with them.
fn state_of(pull_request: PullRequestFacts, reviews: ReviewFacts) -> YourReview {
    let asked = PullRequestFacts {
        review_requests: vec![me()],
        ..pull_request
    };
    let workbench = reviewed_workbench(vec![asked], vec![], reviews);
    let listed = workbench.items.iter().chain(&workbench.quiet);
    let rows = listed.flat_map(|listed| match &listed.item {
        WorkbenchItem::StandalonePr(tree) => Some(tree.row.clone()),
        WorkbenchItem::Issue(_) | WorkbenchItem::StandaloneBackport(_) => None,
    });
    rows.into_iter().next().unwrap().your_review
}

#[test]
fn a_pr_without_the_users_review_is_unreviewed() {
    let pending = PullRequestFacts {
        latest_reviews: vec![LatestReview {
            author: me(),
            state: ReviewState::Pending,
        }],
        ..stranger()
    };

    assert_eq!(
        state_of(pending, ReviewFacts::default()),
        YourReview::Unreviewed
    );
}

#[test]
fn a_review_whose_commit_tsugiki_hasnt_read_yet_is_reviewed() {
    assert_eq!(
        state_of(reviewed_pr(), ReviewFacts::default()),
        YourReview::Reviewed
    );
}

#[test]
fn a_head_equal_to_the_reviewed_commit_is_reviewed_with_no_check() {
    let at_head = PullRequestFacts {
        head: Some(sha('1')),
        ..reviewed_pr()
    };

    assert_eq!(
        state_of(at_head, reviewed_at_1(None, None)),
        YourReview::Reviewed
    );
}

#[test]
fn a_moved_head_stays_reviewed_until_both_sides_have_patch_ids() {
    let reviewed = Some(vec![file("Lib/ast.py", '1', 'a')]);

    assert_eq!(
        state_of(reviewed_pr(), reviewed_at_1(reviewed, None)),
        YourReview::Reviewed
    );
}

#[test]
fn a_moved_head_with_the_same_changes_is_reviewed() {
    let same = || Some(vec![file("Lib/ast.py", '1', 'a')]);

    assert_eq!(
        state_of(reviewed_pr(), reviewed_at_1(same(), same())),
        YourReview::Reviewed
    );
}

#[test]
fn a_moved_head_with_other_changes_is_a_stale_review_naming_the_files() {
    let reviewed = Some(vec![
        file("Lib/ast.py", '1', 'a'),
        file("Lib/os.py", '2', 'b'),
    ]);
    let head = Some(vec![
        file("Lib/ast.py", '3', 'c'),
        file("Lib/os.py", '4', 'b'),
    ]);

    assert_eq!(
        state_of(reviewed_pr(), reviewed_at_1(reviewed, head)),
        YourReview::StaleReview {
            files: vec![
                ChangedFile {
                    path: "Lib/ast.py".to_owned(),
                    whitespace_only: false,
                },
                ChangedFile {
                    path: "Lib/os.py".to_owned(),
                    whitespace_only: true,
                },
            ],
        }
    );
}

/// `reviews`, where PR 1's last check compared commit `reviewed` with commit 3, whose change was `head`.
fn last_checked(mut reviews: ReviewFacts, reviewed: char, head: Vec<FilePatch>) -> ReviewFacts {
    drop(reviews.patches.insert(of('3', "main"), head));
    let last = LastCheck {
        reviewed: sha(reviewed),
        head: sha('3'),
    };
    drop(reviews.last_checks.insert(1, last));
    reviews
}

#[test]
fn a_pending_check_keeps_the_last_verdict() {
    let reviewed = Some(vec![file("Lib/ast.py", '1', 'a')]);
    let last = vec![file("Lib/ast.py", '3', 'a')];
    let reviews = last_checked(reviewed_at_1(reviewed, None), '1', last);

    assert_eq!(
        state_of(reviewed_pr(), reviews),
        YourReview::StaleReview {
            files: vec![ChangedFile {
                path: "Lib/ast.py".to_owned(),
                whitespace_only: true,
            }],
        }
    );
}

#[test]
fn a_pending_check_after_a_new_review_is_reviewed() {
    let reviewed = Some(vec![file("Lib/ast.py", '1', 'a')]);
    let last = vec![file("Lib/ast.py", '3', 'c')];
    let reviews = last_checked(reviewed_at_1(reviewed, None), '9', last);

    assert_eq!(state_of(reviewed_pr(), reviews), YourReview::Reviewed);
}

fn reasons() -> [CantCheck; 5] {
    let version = "git version 2.38.1".to_owned();
    [
        CantCheck::Cloning,
        CantCheck::Broken,
        CantCheck::NoGit,
        CantCheck::OldGit {
            version,
            needed: "2.39".to_owned(),
        },
        CantCheck::Refused,
    ]
}

#[test]
fn a_check_that_cant_run_on_either_side_is_a_stale_review_with_the_reason() {
    for reason in reasons() {
        for side in ['1', '2'] {
            let mut reviews = reviewed_at_1(None, None);
            drop(reviews.unchecked.insert(of(side, "main"), reason.clone()));

            assert_eq!(
                state_of(reviewed_pr(), reviews),
                YourReview::CantCheck {
                    reason: reason.clone()
                },
                "commit {side}"
            );
        }
    }
}

#[test]
fn a_check_that_cant_run_outranks_the_last_verdict() {
    let reviewed = Some(vec![file("Lib/ast.py", '1', 'a')]);
    let last = vec![file("Lib/ast.py", '1', 'a')];
    let mut reviews = last_checked(reviewed_at_1(reviewed, None), '1', last);
    drop(
        reviews
            .unchecked
            .insert(of('2', "main"), CantCheck::Cloning),
    );

    assert_eq!(
        state_of(reviewed_pr(), reviews),
        YourReview::CantCheck {
            reason: CantCheck::Cloning
        }
    );
}

#[test]
fn once_both_sides_are_read_their_verdict_replaces_the_reason() {
    let read = || Some(vec![file("Lib/ast.py", '1', 'a')]);
    let mut reviews = reviewed_at_1(read(), read());
    drop(reviews.unchecked.insert(of('1', "main"), CantCheck::NoGit));

    assert_eq!(state_of(reviewed_pr(), reviews), YourReview::Reviewed);
}

#[test]
fn the_users_comments_on_their_own_pr_arent_a_review_of_it() {
    let reviewed = Some(vec![file("Lib/ast.py", '1', 'a')]);
    let head = Some(vec![file("Lib/ast.py", '3', 'c')]);

    assert_eq!(
        state_of(own(reviewed_pr()), reviewed_at_1(reviewed, head)),
        YourReview::Unreviewed
    );
}

/// The facts of `pull_requests`, which `reviews` says the user reviewed.
fn facts(pull_requests: Vec<PullRequestFacts>, reviews: ReviewFacts) -> WorkFacts {
    WorkFacts {
        viewer: Some(me()),
        pull_requests,
        issues: vec![],
        filters: BTreeMap::new(),
        activity: ActivityFacts::default(),
        reviews,
    }
}

fn of(digit: char, target_branch: &str) -> PatchesOf {
    PatchesOf {
        commit: sha(digit),
        target_branch: target_branch.to_owned(),
    }
}

/// The commits the check asks the mirror to read for the user's work in `facts`.
fn to_read(facts: WorkFacts) -> Vec<PatchesOf> {
    Workbench::mirror_work(facts, &cpython()).1
}

#[test]
fn a_check_reads_both_sides_of_each_open_pr_whose_head_moved_on_from_the_review() {
    let backport = PullRequestFacts {
        number: 3,
        target_branch: "3.14".to_owned(),
        head: Some(sha('4')),
        ..reviewed_pr()
    };
    let at_head = PullRequestFacts {
        number: 5,
        head: Some(sha('5')),
        ..reviewed_pr()
    };
    // A merged or closed PR the user's work doesn't reach shows on no screen.
    let merged = PullRequestFacts {
        number: 6,
        state: PrState::Merged,
        ..reviewed_pr()
    };
    let closed = PullRequestFacts {
        number: 9,
        state: PrState::Closed,
        head: Some(sha('9')),
        ..reviewed_pr()
    };
    let unreviewed = PullRequestFacts {
        number: 7,
        ..stranger()
    };
    let own_pr = own(PullRequestFacts {
        number: 8,
        head: Some(sha('8')),
        ..reviewed_pr()
    });
    let mut reviews = reviewed_at_1(None, None);
    reviews.reviewed.extend([
        (3, sha('1')),
        (5, sha('5')),
        (6, sha('6')),
        (8, sha('7')),
        (9, sha('1')),
    ]);
    let pull_requests = vec![
        reviewed_pr(),
        backport,
        at_head,
        merged,
        unreviewed,
        own_pr,
        closed,
    ];

    assert_eq!(
        to_read(facts(pull_requests, reviews)),
        [
            of('1', "3.14"),
            of('1', "main"),
            of('2', "main"),
            of('4', "3.14")
        ]
    );
}

#[test]
fn a_check_doesnt_read_again_what_the_store_holds() {
    let read = Some(vec![file("Lib/ast.py", '1', 'a')]);
    let reviews = reviewed_at_1(read, None);

    assert_eq!(
        to_read(facts(vec![reviewed_pr()], reviews)),
        [of('2', "main")]
    );
}

#[test]
fn a_check_keeps_the_commits_it_compared_once_both_sides_are_read() {
    let read = || Some(vec![file("Lib/ast.py", '1', 'a')]);
    let kept = PullRequestFacts {
        number: 3,
        ..reviewed_pr()
    };
    let pending = PullRequestFacts {
        number: 5,
        head: Some(sha('4')),
        ..reviewed_pr()
    };
    let mut reviews = reviewed_at_1(read(), read());
    reviews.reviewed.extend([(3, sha('1')), (5, sha('1'))]);
    let last = LastCheck {
        reviewed: sha('1'),
        head: sha('2'),
    };
    drop(reviews.last_checks.insert(3, last.clone()));
    let pull_requests = vec![reviewed_pr(), kept, pending];

    assert_eq!(
        Workbench::checks_to_keep(&facts(pull_requests, reviews)),
        [(1, last)]
    );
}

#[test]
fn a_review_tsugiki_read_from_the_conversation_counts_before_the_list_shows_it() {
    let unlisted = PullRequestFacts {
        latest_reviews: vec![],
        ..reviewed_pr()
    };

    assert_eq!(
        state_of(unlisted, reviewed_at_1(None, None)),
        YourReview::Reviewed
    );
}

#[test]
fn a_merged_prs_check_reads_its_final_head_once_while_the_users_work_reaches_it() {
    let in_issue_10 = |pr| PullRequestFacts {
        title: "gh-10: Fix it".to_owned(),
        ..pr
    };
    let merged = |number| PullRequestFacts {
        number,
        state: PrState::Merged,
        ..reviewed_pr()
    };
    let frozen = PullRequestFacts {
        head: Some(sha('3')),
        ..merged(5)
    };
    // PR 6's Review request keeps Issue 10, and the merged PRs in its tree, on the Workbench.
    let asking = PullRequestFacts {
        number: 6,
        review_requests: vec![me()],
        ..stranger()
    };
    let unreached = PullRequestFacts {
        state: PrState::Closed,
        head: Some(sha('4')),
        ..merged(7)
    };
    let mut reviews = reviewed_at_1(Some(vec![file("Lib/ast.py", '1', 'a')]), None);
    reviews.reviewed.extend([(5, sha('1')), (7, sha('1'))]);
    drop(reviews.patches.insert(of('3', "main"), vec![]));
    let pull_requests = vec![
        in_issue_10(merged(1)),
        in_issue_10(frozen),
        in_issue_10(asking),
        unreached,
    ];
    let facts = WorkFacts {
        issues: vec![issue(10)],
        ..facts(pull_requests, reviews)
    };

    let (heads, patches) = Workbench::mirror_work(facts, &cpython());

    let fetched: Vec<u32> = heads.iter().map(|head| head.number).collect();
    assert_eq!(fetched, [1, 5]);
    // PR 5's check has read both sides, so its verdict is frozen, and nothing of it is read again.
    assert_eq!(patches, [of('2', "main")]);
}

/// The bot's Backport `number` of PR 1, at commit 9 and last updated before PR 1.
fn bot_backport_of_1(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        head: Some(sha('9')),
        updated_at: at(0),
        ..bots_backport(number, "[3.14] gh-1: Fix it (GH-1)")
    }
}

/// The Backports 2, 3 and 4 that the bot made of PR 1, beneath PR 1, which the user reviewed as `reviews`
/// says.
fn origin_and_bot_backports() -> Vec<PullRequestFacts> {
    vec![
        reviewed_pr(),
        bot_backport_of_1(2),
        bot_backport_of_1(3),
        bot_backport_of_1(4),
    ]
}

/// The review state on each row of the Workbench of `pull_requests`.
fn states_of(
    pull_requests: Vec<PullRequestFacts>,
    reviews: ReviewFacts,
) -> BTreeMap<u32, YourReview> {
    let workbench = reviewed_workbench(pull_requests, vec![], reviews);
    let listed = workbench.items.iter().chain(&workbench.quiet);
    let rows = listed.flat_map(|listed| listed.item.rows().into_iter().cloned());
    rows.map(|row| (row.number, row.your_review)).collect()
}

/// PR 1's change at commit 2, which differs from the one the user reviewed at commit 1.
fn stale_at_2() -> ReviewFacts {
    let reviewed = Some(vec![file("Lib/ast.py", '1', 'a')]);
    let head = Some(vec![file("Lib/ast.py", '3', 'c')]);
    reviewed_at_1(reviewed, head)
}

fn ast_changed() -> YourReview {
    YourReview::StaleReview {
        files: vec![ChangedFile {
            path: "Lib/ast.py".to_owned(),
            whitespace_only: false,
        }],
    }
}

#[test]
fn a_bot_backport_shows_its_origin_prs_review_state() {
    let same = || Some(vec![file("Lib/ast.py", '1', 'a')]);
    let reviewed = states_of(origin_and_bot_backports(), reviewed_at_1(same(), same()));
    let stale = states_of(origin_and_bot_backports(), stale_at_2());

    for number in [2, 3, 4] {
        assert_eq!(reviewed[&number], YourReview::Reviewed, "PR {number}");
        assert_eq!(stale[&number], ast_changed(), "PR {number}");
    }
}

#[test]
fn a_bot_backport_shares_its_origin_prs_state_over_a_review_of_its_own() {
    let unreviewed_origin = PullRequestFacts {
        latest_reviews: vec![],
        ..stranger()
    };
    let reviewed_backport = PullRequestFacts {
        latest_reviews: reviewed_pr().latest_reviews,
        ..bot_backport_of_1(2)
    };

    let states = states_of(
        vec![unreviewed_origin, reviewed_backport],
        ReviewFacts::default(),
    );

    assert_eq!(states[&2], YourReview::Unreviewed);
}

#[test]
fn a_stale_origin_pr_with_three_bot_backports_needs_attention_once() {
    let workbench = reviewed_workbench(origin_and_bot_backports(), vec![], stale_at_2());

    assert_eq!(workbench.count, 1);
    let attention = workbench.items[0].attention.clone().unwrap();
    assert_eq!(attention.why, [Why::StaleReview]);
    // The Backports were updated before PR 1, so a Stale review of theirs would start earlier.
    assert_eq!(attention.since, at(1));
    let facts = facts(origin_and_bot_backports(), stale_at_2());
    let causes = Causes::of(&facts, cpython().conventions);
    let waiting: Vec<u32> = (1..=4)
        .filter(|&number| causes.needs_attention(number))
        .collect();
    assert_eq!(waiting, [1]);
}

#[test]
fn a_bot_backport_with_a_commit_by_a_person_gets_its_own_review_state() {
    let bot = Some(Login::new(BOT));
    let manual = PullRequestFacts {
        author: bot.clone(),
        head: Some(sha('9')),
        ..backport(2, "[3.14] gh-1: Fix it (GH-1)", vec![bot, Some(someone())])
    };
    let reviewed = PullRequestFacts {
        latest_reviews: reviewed_pr().latest_reviews,
        ..manual.clone()
    };

    let states = states_of(vec![reviewed_pr(), manual], stale_at_2());
    let reviewed_states = states_of(vec![reviewed_pr(), reviewed], stale_at_2());

    assert_eq!(states[&2], YourReview::Unreviewed);
    assert_eq!(reviewed_states[&2], YourReview::Reviewed);
}

#[test]
fn a_merged_stale_origin_prs_open_bot_backports_show_it_and_wait_on_nobody() {
    let merged = PullRequestFacts {
        state: PrState::Merged,
        ..reviewed_pr()
    };
    // The user's approval of the Backport keeps it on the Workbench.
    let approved = PullRequestFacts {
        latest_reviews: reviewed_pr().latest_reviews,
        ..bot_backport_of_1(2)
    };
    let pull_requests = vec![merged, approved];

    let workbench = reviewed_workbench(pull_requests.clone(), vec![], stale_at_2());

    assert_eq!(states_of(pull_requests, stale_at_2())[&2], ast_changed());
    assert_eq!(workbench.count, 0);
}

#[test]
fn a_bot_backport_whose_origin_pr_tsugiki_lacks_keeps_its_own_review_state() {
    let alone = PullRequestFacts {
        latest_reviews: reviewed_pr().latest_reviews,
        ..bot_backport_of_1(2)
    };

    assert_eq!(
        states_of(vec![alone], ReviewFacts::default())[&2],
        YourReview::Reviewed
    );
}

#[test]
fn a_review_request_on_a_bot_backport_needs_attention_whatever_its_origin_prs_state() {
    let asked = PullRequestFacts {
        review_requests: vec![me()],
        ..bot_backport_of_1(2)
    };
    let same = || Some(vec![file("Lib/ast.py", '1', 'a')]);

    for reviews in [reviewed_at_1(same(), same()), stale_at_2()] {
        let facts = facts(vec![reviewed_pr(), asked.clone()], reviews);
        let causes = Causes::of(&facts, cpython().conventions);

        assert!(causes.needs_attention(2));
    }
}

/// The Stale reviews view of `pull_requests` and `issues`, which the user reviewed as `reviews` says.
fn stale_view(
    pull_requests: Vec<PullRequestFacts>,
    issues: Vec<IssueFacts>,
    reviews: ReviewFacts,
) -> Workbench {
    let facts = WorkFacts {
        issues,
        ..facts(pull_requests, reviews)
    };
    Workbench::from_facts(facts, &cpython(), &WorkbenchView::StaleReviews)
}

/// PR `number` like PR 1, which the user reviewed at commit 1, at `head` and fixing Issue `issue`.
fn reviewed_at(number: u32, head: char, issue: u32) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("gh-{issue}: Fix it"),
        head: Some(sha(head)),
        ..reviewed_pr()
    }
}

#[test]
fn the_stale_reviews_view_lists_the_open_prs_with_a_stale_review_in_their_issues() {
    let ended = |number, state| PullRequestFacts {
        state,
        ..reviewed_at(number, '2', 20)
    };
    let unchecked = PullRequestFacts {
        title: "Tidy up".to_owned(),
        ..reviewed_at(6, '5', 0)
    };
    let draft = PullRequestFacts {
        is_draft: true,
        ..reviewed_at(8, '2', 30)
    };
    let unreviewed = PullRequestFacts {
        number: 7,
        title: "gh-20: Fix it".to_owned(),
        ..stranger()
    };
    let pull_requests = vec![
        reviewed_at(1, '2', 10),
        reviewed_at(2, '1', 20),
        ended(3, PrState::Merged),
        ended(4, PrState::Closed),
        unchecked,
        unreviewed,
        draft,
    ];
    let mut reviews = stale_at_2();
    reviews
        .reviewed
        .extend([2, 3, 4, 6, 8].map(|number| (number, sha('1'))));
    drop(
        reviews
            .unchecked
            .insert(of('5', "main"), CantCheck::Cloning),
    );
    let issues = vec![issue(10), issue(20), issue(30)];

    let view = stale_view(pull_requests.clone(), issues.clone(), reviews.clone());
    let whole = Workbench::from_facts(
        WorkFacts {
            issues,
            ..facts(pull_requests, reviews)
        },
        &cpython(),
        &WorkbenchView::Whole,
    );

    // A colleague's Draft PR waits on nobody, as for Needs attention.
    assert_eq!(
        outline(&view),
        [("issue", 10, vec![1]), ("standalone", 6, vec![])]
    );
    assert_eq!(view.stale_reviews, 2);
    assert_eq!(whole.stale_reviews, 2);
}

#[test]
fn a_stale_origin_pr_and_its_bot_backports_count_once_in_the_stale_reviews_view() {
    let view = stale_view(origin_and_bot_backports(), vec![], stale_at_2());

    assert_eq!(outline(&view), [("standalone", 1, vec![2, 3, 4])]);
    assert_eq!(view.stale_reviews, 1);
}

#[test]
fn a_merged_stale_origin_prs_open_bot_backports_stay_out_of_the_stale_reviews_view() {
    let merged = PullRequestFacts {
        state: PrState::Merged,
        ..reviewed_pr()
    };
    let approved = PullRequestFacts {
        latest_reviews: reviewed_pr().latest_reviews,
        ..bot_backport_of_1(2)
    };

    let view = stale_view(vec![merged, approved], vec![], stale_at_2());

    assert_eq!(outline(&view), []);
    assert_eq!(view.stale_reviews, 0);
}

#[test]
fn a_pr_leaves_the_stale_reviews_view_once_it_merges_or_turns_reviewed() {
    let merged = PullRequestFacts {
        state: PrState::Merged,
        ..reviewed_pr()
    };
    let mut reviewed_again = stale_at_2();
    drop(reviewed_again.reviewed.insert(1, sha('2')));

    let stale = stale_view(vec![reviewed_pr()], vec![], stale_at_2());
    let after_merge = stale_view(vec![merged], vec![], stale_at_2());
    let after_review = stale_view(vec![reviewed_pr()], vec![], reviewed_again);

    assert_eq!(stale.stale_reviews, 1);
    for view in [after_merge, after_review] {
        assert_eq!(outline(&view), []);
        assert_eq!(view.stale_reviews, 0);
    }
}

#[test]
fn a_check_leaves_out_a_bot_backport_that_shares_its_origin_prs_state() {
    let manual = PullRequestFacts {
        head: Some(sha('8')),
        ..backport(3, "[3.14] gh-1: Fix it (GH-1)", vec![Some(someone())])
    };
    let mut reviews = reviewed_at_1(None, None);
    reviews.reviewed.extend([(2, sha('7')), (3, sha('7'))]);
    let pull_requests = vec![reviewed_pr(), bot_backport_of_1(2), manual];

    assert_eq!(
        to_read(facts(pull_requests, reviews)),
        [
            of('1', "main"),
            of('2', "main"),
            of('7', "3.14"),
            of('8', "3.14")
        ]
    );
}
