//! A PR's review state on its row, from the user's Review and the patch-ids of both sides.

use std::collections::BTreeMap;

use crate::github::CommitSha;
use crate::github::pull_requests::{LatestReview, PrState, PullRequestFacts, ReviewState};
use crate::patch_ids::{ChangedFile, FilePatch, PatchId, PatchesOf};
use crate::store::{ActivityFacts, ReviewFacts, WorkFacts};
use crate::workbench::testing::{me, own, reviewed_workbench, stranger};
use crate::workbench::{Workbench, WorkbenchItem, YourReview};

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
    let merged = PullRequestFacts {
        number: 6,
        state: PrState::Merged,
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
    reviews
        .reviewed
        .extend([(3, sha('1')), (5, sha('5')), (6, sha('6')), (8, sha('7'))]);
    let pull_requests = vec![reviewed_pr(), backport, at_head, merged, unreviewed, own_pr];

    assert_eq!(
        Workbench::patches_to_read(&facts(pull_requests, reviews)),
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
        Workbench::patches_to_read(&facts(vec![reviewed_pr()], reviews)),
        [of('2', "main")]
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
