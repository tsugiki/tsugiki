use std::fs;
use std::path::PathBuf;

use time::OffsetDateTime;

use super::*;
use crate::github::issue_content::IssueContent;
use crate::github::pull_request_content::{PullRequestContent, Review};
use crate::github::pull_requests::{PrState, ReviewState};
use crate::login::Login;
use crate::patch_ids::PatchId;
use crate::store::Store;
use crate::store::testing::temp_database;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn me() -> Login {
    Login::new("octocat")
}

fn sha(digit: char) -> CommitSha {
    CommitSha::parse(&digit.to_string().repeat(40)).unwrap()
}

fn file(path: &str, digit: char) -> FilePatch {
    let id = PatchId::parse(&digit.to_string().repeat(40)).unwrap();
    FilePatch {
        path: path.to_owned(),
        verbatim: id.clone(),
        loose: id,
        generated: digit == 'f',
    }
}

fn of(commit: &CommitSha, target_branch: &str) -> PatchesOf {
    PatchesOf {
        commit: commit.clone(),
        target_branch: target_branch.to_owned(),
    }
}

/// A colleague's open PR into main at `head`.
fn pull_request(number: u32, head: &CommitSha) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("PR {number}"),
        state: PrState::Open,
        is_draft: false,
        author: Some(Login::new("some-author")),
        target_branch: "main".to_owned(),
        ci: None,
        updated_at: OffsetDateTime::UNIX_EPOCH,
        review_requests: vec![],
        latest_reviews: vec![],
        assignees: vec![],
        labels: vec![],
        closing_issues: vec![],
        merge_commit: None,
        base: None,
        head: Some(head.clone()),
        off_default: None,
    }
}

/// A PR's Conversation with the user's Review of `commit`.
fn reviewed_at(commit: &CommitSha) -> PullRequestContent {
    PullRequestContent {
        conversation: IssueContent {
            body_html: String::new(),
            comments: vec![],
            earlier_comments: 0,
            opened_at: None,
        },
        reviews: vec![Review {
            id: None,
            author: Some(me()),
            state: ReviewState::Approved,
            commit: Some(commit.clone()),
            submitted_at: OffsetDateTime::UNIX_EPOCH,
            body_html: String::new(),
            hidden: None,
            threads: vec![],
        }],
        earlier_reviews: 0,
        head: Some(commit.clone()),
        pending: vec![],
    }
}

/// A store with the user signed in, and PR 1 reviewed at commit 1 and now at commit 2.
fn store(name: &str) -> (Store, PathBuf) {
    let (path, folder) = temp_database(name);
    let store = Store::open(&path).unwrap();
    store.remember_login(&me()).unwrap();
    store
        .save_pull_requests(&cpython(), vec![pull_request(1, &sha('2'))])
        .unwrap();
    store
        .save_pull_request_content(&cpython(), 1, reviewed_at(&sha('1')))
        .unwrap();
    (store, folder)
}

fn finish(store: Store, folder: PathBuf) {
    store.close();
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_facts_hold_the_reviewed_commit_and_the_patch_ids_of_both_sides_once_read() {
    let (store, folder) = store("patch-ids-both");
    let unread = store.work_facts(&cpython()).unwrap().reviews;
    let reviewed = vec![file("Lib/ast.py", '1'), file("Python/clinic.h", 'f')];
    let head = vec![file("Lib/ast.py", '2')];
    store
        .save_patch_ids(
            &cpython(),
            vec![
                CommitPatches {
                    of: of(&sha('1'), "main"),
                    files: reviewed.clone(),
                },
                CommitPatches {
                    of: of(&sha('2'), "main"),
                    files: head.clone(),
                },
            ],
        )
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews;
    finish(store, folder);

    assert_eq!(
        unread,
        ReviewFacts {
            reviewed: BTreeMap::from([(1, sha('1'))]),
            ..ReviewFacts::default()
        }
    );
    assert_eq!(
        read,
        ReviewFacts {
            reviewed: BTreeMap::from([(1, sha('1'))]),
            patches: BTreeMap::from([
                (of(&sha('1'), "main"), reviewed),
                (of(&sha('2'), "main"), head),
            ]),
            ..ReviewFacts::default()
        }
    );
}

#[test]
fn a_commit_with_nothing_to_diff_is_held_with_no_files() {
    let (store, folder) = store("patch-ids-empty");
    let empty = CommitPatches {
        of: of(&sha('2'), "main"),
        files: vec![],
    };
    store.save_patch_ids(&cpython(), vec![empty]).unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    finish(store, folder);

    assert_eq!(read, BTreeMap::from([(of(&sha('2'), "main"), vec![])]));
}

#[test]
fn reading_a_commit_again_replaces_what_was_held() {
    let (store, folder) = store("patch-ids-again");
    let first = CommitPatches {
        of: of(&sha('2'), "main"),
        files: vec![file("Lib/ast.py", '1'), file("Lib/os.py", '3')],
    };
    let again = CommitPatches {
        files: vec![file("Lib/ast.py", '2')],
        ..first.clone()
    };
    store.save_patch_ids(&cpython(), vec![first]).unwrap();
    store.save_patch_ids(&cpython(), vec![again]).unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    finish(store, folder);

    assert_eq!(
        read,
        BTreeMap::from([(of(&sha('2'), "main"), vec![file("Lib/ast.py", '2')])])
    );
}

#[test]
fn patch_ids_against_another_branch_or_of_a_commit_no_check_compares_stay_out() {
    let (store, folder) = store("patch-ids-other");
    let elsewhere = [of(&sha('2'), "3.14"), of(&sha('3'), "main")].map(|of| CommitPatches {
        of,
        files: vec![file("Lib/ast.py", '1')],
    });
    store
        .save_patch_ids(&cpython(), elsewhere.to_vec())
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    finish(store, folder);

    assert_eq!(read, BTreeMap::new());
}

fn unchecked(commit: char, reason: CantCheck) -> UncheckedCommit {
    UncheckedCommit {
        of: of(&sha(commit), "main"),
        reason,
    }
}

#[test]
fn the_commits_the_worker_cant_read_replace_those_it_couldnt_before() {
    let (store, folder) = store("patch-ids-unchecked");
    let old_git = CantCheck::OldGit {
        version: "git version 2.38.1".to_owned(),
        needed: "2.39".to_owned(),
    };
    let before = vec![
        unchecked('1', CantCheck::Refused),
        unchecked('2', CantCheck::Cloning),
    ];
    store.save_unchecked(&cpython(), before).unwrap();
    let first = store.work_facts(&cpython()).unwrap().reviews.unchecked;
    let now = vec![unchecked('2', old_git.clone())];
    store.save_unchecked(&cpython(), now).unwrap();

    let then = store.work_facts(&cpython()).unwrap().reviews.unchecked;
    finish(store, folder);

    assert_eq!(
        first,
        BTreeMap::from([
            (of(&sha('1'), "main"), CantCheck::Refused),
            (of(&sha('2'), "main"), CantCheck::Cloning),
        ])
    );
    assert_eq!(then, BTreeMap::from([(of(&sha('2'), "main"), old_git)]));
}

#[test]
fn each_reason_comes_back_as_it_was_kept() {
    let (store, folder) = store("patch-ids-reasons");
    let reasons = [
        CantCheck::Broken,
        CantCheck::NoGit,
        CantCheck::OldGit {
            version: "git version 2.29.2".to_owned(),
            needed: "2.30".to_owned(),
        },
    ];
    let mut read = vec![];
    for reason in reasons.clone() {
        store
            .save_unchecked(&cpython(), vec![unchecked('2', reason)])
            .unwrap();
        let facts = store.work_facts(&cpython()).unwrap();
        read.extend(facts.reviews.unchecked.into_values());
    }
    finish(store, folder);

    assert_eq!(read, reasons);
}

#[test]
fn a_new_run_forgets_why_the_worker_couldnt_read_patch_ids() {
    let (store, folder) = store("patch-ids-forget");
    store
        .save_unchecked(&cpython(), vec![unchecked('2', CantCheck::Cloning)])
        .unwrap();

    store.forget_unchecked().unwrap();
    let read = store.work_facts(&cpython()).unwrap().reviews.unchecked;
    finish(store, folder);

    assert_eq!(read, BTreeMap::new());
}

#[test]
fn a_reason_the_store_doesnt_know_fails_the_read() {
    let (store, folder) = store("patch-ids-unknown-reason");
    store
        .save_unchecked(&cpython(), vec![unchecked('2', CantCheck::NoGit)])
        .unwrap();
    // Only a file from a newer Tsugiki, or one changed by hand, says this, which our functions refuse.
    let changed = store
        .call(|connection| {
            connection.execute(
                "UPDATE unchecked_commits SET git_version = 'git version 2.0'",
                [],
            )
        })
        .unwrap();
    assert_eq!(changed, Ok(1));

    let read = store.work_facts(&cpython());
    finish(store, folder);

    assert!(read.is_err(), "{read:?}");
}

#[test]
fn the_last_check_of_a_pr_brings_the_patch_ids_of_the_head_it_compared() {
    let (store, folder) = store("patch-ids-last-check");
    let earlier = CommitPatches {
        of: of(&sha('3'), "main"),
        files: vec![file("Lib/ast.py", '3')],
    };
    store.save_patch_ids(&cpython(), vec![earlier]).unwrap();
    let first = LastCheck {
        reviewed: sha('1'),
        head: sha('4'),
    };
    let last = LastCheck {
        reviewed: sha('1'),
        head: sha('3'),
    };
    store
        .save_last_checks(&cpython(), vec![(1, first)])
        .unwrap();
    store
        .save_last_checks(&cpython(), vec![(1, last.clone())])
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews;
    finish(store, folder);

    assert_eq!(read.last_checks, BTreeMap::from([(1, last)]));
    assert_eq!(
        read.patches,
        BTreeMap::from([(of(&sha('3'), "main"), vec![file("Lib/ast.py", '3')])])
    );
}

#[test]
fn the_last_check_of_another_review_brings_no_patch_ids() {
    let (store, folder) = store("patch-ids-last-other");
    let earlier = CommitPatches {
        of: of(&sha('3'), "main"),
        files: vec![file("Lib/ast.py", '3')],
    };
    store.save_patch_ids(&cpython(), vec![earlier]).unwrap();
    let other = LastCheck {
        reviewed: sha('9'),
        head: sha('3'),
    };
    store
        .save_last_checks(&cpython(), vec![(1, other)])
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    finish(store, folder);

    assert_eq!(read, BTreeMap::new());
}
