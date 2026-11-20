use time::OffsetDateTime;

use super::*;
use crate::facts::issue_content::IssueContent;
use crate::facts::patch_ids::PatchId;
use crate::facts::pull_request_content::{PullRequestContent, Review};
use crate::facts::pull_requests::ReviewState;
use crate::login::Login;
use crate::store::Store;
use crate::store::testing::{at_second, database_before, migrated_database};
use crate::testing::{TempPath, cpython, me, pr_facts, sha_of};

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
        author: Some(Login::new("some-author")),
        head: Some(head.clone()),
        ..pr_facts(number)
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

/// A store of a database with the user signed in, and PR 1 reviewed at commit 1 and now at commit 2, and
/// the temp folder that holds it. Close the store before the folder drops: bound as `(_folder, store)`, the
/// store drops first on a panic too.
fn seeded(name: &str) -> (TempPath, Store) {
    let (path, folder) = migrated_database(name);
    let store = Store::open(&path).unwrap();
    store.remember_login(&me()).unwrap();
    store
        .save_pull_requests(&cpython(), vec![pull_request(1, &sha_of('2'))])
        .unwrap();
    store
        .save_pull_request_content(
            &cpython(),
            1,
            (reviewed_at(&sha_of('1')), OffsetDateTime::UNIX_EPOCH),
        )
        .unwrap();
    (folder, store)
}

#[test]
fn the_facts_hold_the_reviewed_commit_and_the_patch_ids_of_both_sides_once_read() {
    let (_folder, store) = seeded("patch-ids-both");
    let unread = store.work_facts(&cpython()).unwrap().reviews;
    let reviewed = vec![file("Lib/ast.py", '1'), file("Python/clinic.h", 'f')];
    let head = vec![file("Lib/ast.py", '2')];
    store
        .save_patch_ids(
            &cpython(),
            vec![
                CommitPatches {
                    of: of(&sha_of('1'), "main"),
                    files: reviewed.clone(),
                },
                CommitPatches {
                    of: of(&sha_of('2'), "main"),
                    files: head.clone(),
                },
            ],
        )
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews;
    store.close();

    assert_eq!(
        unread,
        ReviewFacts {
            reviewed: BTreeMap::from([(1, sha_of('1'))]),
            ..ReviewFacts::default()
        }
    );
    assert_eq!(
        read,
        ReviewFacts {
            reviewed: BTreeMap::from([(1, sha_of('1'))]),
            patches: BTreeMap::from([
                (of(&sha_of('1'), "main"), reviewed),
                (of(&sha_of('2'), "main"), head),
            ]),
            ..ReviewFacts::default()
        }
    );
}

#[test]
fn a_commit_with_nothing_to_diff_is_held_with_no_files() {
    let (_folder, store) = seeded("patch-ids-empty");
    let empty = CommitPatches {
        of: of(&sha_of('2'), "main"),
        files: vec![],
    };
    store.save_patch_ids(&cpython(), vec![empty]).unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    store.close();

    assert_eq!(read, BTreeMap::from([(of(&sha_of('2'), "main"), vec![])]));
}

#[test]
fn reading_a_commit_again_replaces_what_was_held() {
    let (_folder, store) = seeded("patch-ids-again");
    let first = CommitPatches {
        of: of(&sha_of('2'), "main"),
        files: vec![file("Lib/ast.py", '1'), file("Lib/os.py", '3')],
    };
    let again = CommitPatches {
        files: vec![file("Lib/ast.py", '2')],
        ..first.clone()
    };
    store.save_patch_ids(&cpython(), vec![first]).unwrap();
    store.save_patch_ids(&cpython(), vec![again]).unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    store.close();

    assert_eq!(
        read,
        BTreeMap::from([(of(&sha_of('2'), "main"), vec![file("Lib/ast.py", '2')])])
    );
}

#[test]
fn patch_ids_against_another_branch_or_of_a_commit_no_check_compares_stay_out() {
    let (_folder, store) = seeded("patch-ids-other");
    let elsewhere = [of(&sha_of('2'), "3.14"), of(&sha_of('3'), "main")].map(|of| CommitPatches {
        of,
        files: vec![file("Lib/ast.py", '1')],
    });
    store
        .save_patch_ids(&cpython(), elsewhere.to_vec())
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    store.close();

    assert_eq!(read, BTreeMap::new());
}

#[test]
fn a_save_of_unchecked_commits_or_last_checks_that_fails_says_so() {
    let store = Store::stopped();
    let last = LastCheck {
        reviewed: sha_of('1'),
        head: sha_of('2'),
    };

    let unchecked = store.save_unchecked(&cpython(), vec![]);
    let checks = store.save_last_checks(&cpython(), vec![(1, last)]);
    let findings = store.save_findings(&cpython(), Findings::default(), OffsetDateTime::UNIX_EPOCH);

    assert!(unchecked.is_err(), "{unchecked:?}");
    assert!(checks.is_err(), "{checks:?}");
    assert!(findings.is_err(), "{findings:?}");
}

fn unchecked(commit: char, reason: CantCheck) -> UncheckedCommit {
    UncheckedCommit {
        of: of(&sha_of(commit), "main"),
        reason,
    }
}

#[test]
fn the_commits_the_worker_cant_read_replace_those_it_couldnt_before() {
    let (_folder, store) = seeded("patch-ids-unchecked");
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
    store.close();

    assert_eq!(
        first,
        BTreeMap::from([
            (of(&sha_of('1'), "main"), CantCheck::Refused),
            (of(&sha_of('2'), "main"), CantCheck::Cloning),
        ])
    );
    assert_eq!(then, BTreeMap::from([(of(&sha_of('2'), "main"), old_git)]));
}

#[test]
fn each_reason_comes_back_as_it_was_kept() {
    let (_folder, store) = seeded("patch-ids-reasons");
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
    store.close();

    assert_eq!(read, reasons);
}

#[test]
fn a_new_run_forgets_why_the_worker_couldnt_read_patch_ids() {
    let (_folder, store) = seeded("patch-ids-forget");
    store
        .save_unchecked(&cpython(), vec![unchecked('2', CantCheck::Cloning)])
        .unwrap();

    store.forget_unchecked().unwrap();
    let read = store.work_facts(&cpython()).unwrap().reviews.unchecked;
    store.close();

    assert_eq!(read, BTreeMap::new());
}

#[test]
fn a_reason_the_store_doesnt_know_fails_the_read() {
    let (_folder, store) = seeded("patch-ids-unknown-reason");
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
    store.close();

    assert!(read.is_err(), "{read:?}");
}

#[test]
fn the_last_check_of_a_pr_brings_the_patch_ids_of_the_head_it_compared() {
    let (_folder, store) = seeded("patch-ids-last-check");
    let earlier = CommitPatches {
        of: of(&sha_of('3'), "main"),
        files: vec![file("Lib/ast.py", '3')],
    };
    store.save_patch_ids(&cpython(), vec![earlier]).unwrap();
    let first = LastCheck {
        reviewed: sha_of('1'),
        head: sha_of('4'),
    };
    let last = LastCheck {
        reviewed: sha_of('1'),
        head: sha_of('3'),
    };
    store
        .save_last_checks(&cpython(), vec![(1, first)])
        .unwrap();
    store
        .save_last_checks(&cpython(), vec![(1, last.clone())])
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews;
    store.close();

    assert_eq!(read.last_checks, BTreeMap::from([(1, last)]));
    assert_eq!(
        read.patches,
        BTreeMap::from([(of(&sha_of('3'), "main"), vec![file("Lib/ast.py", '3')])])
    );
}

#[test]
fn the_last_check_of_another_review_brings_no_patch_ids() {
    let (_folder, store) = seeded("patch-ids-last-other");
    let earlier = CommitPatches {
        of: of(&sha_of('3'), "main"),
        files: vec![file("Lib/ast.py", '3')],
    };
    store.save_patch_ids(&cpython(), vec![earlier]).unwrap();
    let other = LastCheck {
        reviewed: sha_of('9'),
        head: sha_of('3'),
    };
    store
        .save_last_checks(&cpython(), vec![(1, other)])
        .unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.patches;
    store.close();

    assert_eq!(read, BTreeMap::new());
}

#[test]
fn a_finding_keeps_its_moment_until_it_ends_and_a_new_review_replaces_it() {
    let (_folder, store) = seeded("patch-ids-findings");
    let found = |numbers: &[u32], commit: char| Findings {
        found: numbers
            .iter()
            .map(|number| (*number, sha_of(commit)))
            .collect(),
        ended: vec![],
    };
    let stale_found = || store.work_facts(&cpython()).unwrap().reviews.stale_found;

    store
        .save_findings(&cpython(), found(&[1, 2], '1'), at_second(10))
        .unwrap();
    let first = stale_found();
    store
        .mark_seen(&cpython(), vec![1, 2], Some(at_second(15)))
        .unwrap();
    let marked = stale_found();
    store
        .save_findings(&cpython(), found(&[2], '3'), at_second(20))
        .unwrap();
    let ended = Findings {
        found: vec![],
        ended: vec![1],
    };
    store
        .save_findings(&cpython(), ended, at_second(30))
        .unwrap();
    let after = stale_found();
    store.close();

    let at = |commit: char, at: i64, acknowledged: bool| StaleFound {
        reviewed: sha_of(commit),
        at: at_second(at),
        acknowledged,
    };
    let unmarked = at('1', 10, false);
    assert_eq!(
        first,
        BTreeMap::from([(1, unmarked.clone()), (2, unmarked)])
    );
    let acknowledged = at('1', 10, true);
    assert_eq!(
        marked,
        BTreeMap::from([(1, acknowledged.clone()), (2, acknowledged)])
    );
    assert_eq!(after, BTreeMap::from([(2, at('3', 20, false))]));
}

#[test]
fn a_mark_with_no_moment_acknowledges_no_finding() {
    let (_folder, store) = seeded("patch-ids-findings-no-moment");
    let found = Findings {
        found: vec![(1, sha_of('1'))],
        ended: vec![],
    };
    store
        .save_findings(&cpython(), found, at_second(10))
        .unwrap();

    store.mark_seen(&cpython(), vec![1], None).unwrap();

    let read = store.work_facts(&cpython()).unwrap().reviews.stale_found;
    store.close();
    assert!(!read[&1].acknowledged);
}

#[test]
fn each_review_of_a_moved_head_before_the_migration_counts_as_found_long_ago_and_acknowledged() {
    // PR 1 moved on from the user's Review. PR 2's head is the commit they reviewed. On PR 3 a colleague
    // reviewed after them.
    let (path, _folder) = database_before(
        "patch-ids-before-findings",
        super::STALE_REVIEWS,
        "INSERT INTO viewer (id, login) VALUES (1, 'octocat');
         INSERT INTO pull_requests (repository, number, title, state, is_draft, author, target_branch, ci,
                                    updated_at, head)
         VALUES ('python/cpython', 1, 'PR 1', 'OPEN', 0, 'some-author', 'main', NULL, 1, '22222222'),
                ('python/cpython', 2, 'PR 2', 'OPEN', 0, 'some-author', 'main', NULL, 1, '33333333'),
                ('python/cpython', 3, 'PR 3', 'OPEN', 0, 'some-author', 'main', NULL, 1, '44444444');
         INSERT INTO issue_content (repository, number, body_html, earlier_comments)
         VALUES ('python/cpython', 1, '', 0), ('python/cpython', 2, '', 0), ('python/cpython', 3, '', 0);
         INSERT INTO pr_reviews (repository, number, position, author, state, commit_sha, submitted_at,
                                 body_html)
         VALUES ('python/cpython', 1, 0, 'octocat', 'APPROVED', '11111111', 1, ''),
                ('python/cpython', 2, 0, 'octocat', 'APPROVED', '33333333', 1, ''),
                ('python/cpython', 3, 0, 'octocat', 'APPROVED', '11111111', 1, ''),
                ('python/cpython', 3, 1, 'some-reviewer', 'APPROVED', '44444444', 2, '');",
    );

    let store = Store::open(&path).unwrap();
    let read = store.work_facts(&cpython()).unwrap().reviews.stale_found;
    store.close();

    let found = StaleFound {
        reviewed: CommitSha::parse("11111111").unwrap(),
        at: OffsetDateTime::UNIX_EPOCH,
        acknowledged: true,
    };
    assert_eq!(read, BTreeMap::from([(1, found.clone()), (3, found)]));
}
